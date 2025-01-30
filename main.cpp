#include <windows.h>
#include <d2d1.h>
#include <dwrite.h>
#include <string>
#include <iostream>
#include <stdexcept>
#include <vector>
#include <wrl/client.h>

using Microsoft::WRL::ComPtr;

inline void ThrowIfFailed(HRESULT hr, const char* message) {
    if (FAILED(hr)) {
        throw std::runtime_error(message);
    }
}

struct ComInit {
    ComInit() {
        HRESULT hr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
        if (FAILED(hr)) {
            throw std::runtime_error("Failed to CoInitializeEx");
        }
    }

    ~ComInit() {
        CoUninitialize();
    }

    // Non-copyable, non-movable
    ComInit(const ComInit &) = delete;
    ComInit &operator=(const ComInit &) = delete;
    ComInit(ComInit &&) = delete;
    ComInit &operator=(ComInit &&) = delete;
};

static constexpr const wchar_t *CLASS_NAME = L"YasWindowClass";
static constexpr const wchar_t *WINDOW_NAME = L"YAS!";

const float triSize = 16.0f;
const float smallFontSize = 12.0f;
const float bigFontSize = 24.0f;
const float border = 4.0f;

static LRESULT CALLBACK WindowProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam);

class App {
public:
    App(HINSTANCE hInstance) {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        texts_.push_back(L"YAS!");

        ThrowIfFailed(
            D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, d2dFactory_.GetAddressOf()),
            "Failed to create Direct2D factory"
        );

        ThrowIfFailed(
            DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                                reinterpret_cast<IUnknown **>(writeFactory_.GetAddressOf())),
            "Failed to create DirectWrite factory"
        );

        wchar_t localeName[LOCALE_NAME_MAX_LENGTH] = {};
        if (0 == GetUserDefaultLocaleName(localeName, LOCALE_NAME_MAX_LENGTH)) {
            wcscpy_s(localeName, L"en-us");
        }

        ThrowIfFailed(
            writeFactory_->CreateTextFormat(
                WINDOW_NAME,
                nullptr,
                DWRITE_FONT_WEIGHT_REGULAR,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                smallFontSize,
                localeName,
                textFormat_.GetAddressOf()),
            "Failed to create small textFormat"
        );
        ThrowIfFailed(
            writeFactory_->CreateTextFormat(
                WINDOW_NAME,
                nullptr,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                bigFontSize,
                localeName,
                bigTextFormat_.GetAddressOf()),
            "Failed to create big textFormat"
        );

        WNDCLASSEXW wc{};
        wc.cbSize = sizeof(wc);
        wc.hInstance = hInstance;
        wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
        wc.lpfnWndProc = WindowProc;
        wc.lpszClassName = CLASS_NAME;
        auto atom = RegisterClassExW(&wc);
        if (atom == 0) {
            throw std::runtime_error("RegisterClassExW failed");
        }

        POINT pt;
        GetCursorPos(&pt);

        HMONITOR hMonitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST); // or MONITOR_DEFAULTTOPRIMARY
        MONITORINFO mi{};
        mi.cbSize = sizeof(mi);
        if (!GetMonitorInfoW(hMonitor, &mi)) {
            throw std::runtime_error("GetMonitorInfoW failed");
        }
        int x = mi.rcMonitor.left;
        int y = mi.rcMonitor.top;
        int w = mi.rcMonitor.right - mi.rcMonitor.left;
        int h = mi.rcMonitor.bottom - mi.rcMonitor.top;

        hwnd_ = CreateWindowExW(
            0,
            CLASS_NAME,
            WINDOW_NAME,
            WS_POPUP | WS_VISIBLE,
            x,
            y,
            w,
            h,
            nullptr,
            nullptr,
            hInstance,
            this);

        if (!hwnd_) {
            throw std::runtime_error("CreateWindowExW failed");
        }

        RECT rc{};
        GetClientRect(hwnd_, &rc);
        ThrowIfFailed(
            d2dFactory_->CreateHwndRenderTarget(
                D2D1::RenderTargetProperties(),
                D2D1::HwndRenderTargetProperties(hwnd_,
                    D2D1::SizeU(rc.right - rc.left, rc.bottom - rc.top)),
                &renderTarget_),
            "Failed to create HwndRenderTarget"
        );

        ThrowIfFailed(
            renderTarget_->CreateSolidColorBrush(
                D2D1::ColorF(1, 1, 1, 1), // white
                &brush_),
            "Failed to create white brush"
        );
        ThrowIfFailed(
            renderTarget_->CreateSolidColorBrush(
                D2D1::ColorF(1, 0, 0, 1), // red
                &redBrush_),
            "Failed to create red brush"
        );

        ThrowIfFailed(
            renderTarget_->CreateSolidColorBrush(
                D2D1::ColorF(0.666f, 0.666f, 0.666f, 1.0f),
                &brushGray_),
            "Failed to create gray brush"
        );

        ShowWindow(hwnd_, SW_SHOW);
        UpdateTextLayouts();
    }

    void UpdateTextLayouts() {
        if (!renderTarget_) {
            return;
        }

        layoutsBig_.clear();
        auto sizeRT = renderTarget_->GetSize();

        for (auto &txt : texts_) {
            ComPtr<IDWriteTextLayout> layout;
            ThrowIfFailed(
                writeFactory_->CreateTextLayout(
                    txt.c_str(),
                    static_cast<UINT32>(txt.size()),
                    bigTextFormat_.Get(),
                    sizeRT.width,
                    sizeRT.height,
                    &layout
                ),
                "Failed to create text layout"
            );
            layoutsBig_.push_back(layout);
        }
    }

    void Draw() {
        if (!renderTarget_)
            return;
        renderTarget_->BeginDraw();
        renderTarget_->Clear(D2D1::ColorF(0, 0, 0, 1));

        float xOffset = border;
        float yOffset = border;

        float maxHeight = 0.0f;

        for (size_t i = 0; i < layoutsBig_.size(); i++) {
            ComPtr<IDWriteTextLayout> &layout = layoutsBig_[i];
            DWRITE_TEXT_METRICS metrics{};
            layout->GetMetrics(&metrics);

            if (metrics.height > maxHeight) {
                maxHeight = metrics.height;
            }

            if (i == activeTextIndex_) {
                brush_->SetColor(D2D1::ColorF(1, 1, 1, 1));
                renderTarget_->DrawTextLayout(
                    D2D1::Point2F(xOffset, yOffset),
                    layout.Get(),
                    brush_.Get());
            } else {
                brushGray_->SetColor(D2D1::ColorF(0.666f, 0.666f, 0.666f, 1.0f));
                renderTarget_->DrawTextLayout(
                    D2D1::Point2F(xOffset, yOffset),
                    layout.Get(),
                    brushGray_.Get());
            }

            xOffset += metrics.widthIncludingTrailingWhitespace;

            ComPtr<ID2D1PathGeometry> geometry;
            ThrowIfFailed(d2dFactory_->CreatePathGeometry(&geometry), "CreatePathGeometry failed");

            ComPtr<ID2D1GeometrySink> sink;
            ThrowIfFailed(geometry->Open(&sink), "Opening geometry sink failed");

            sink->BeginFigure(D2D1::Point2F(xOffset, yOffset), D2D1_FIGURE_BEGIN_FILLED);
            sink->AddLine(D2D1::Point2F(xOffset + triSize, yOffset + metrics.height / 2.0f));
            sink->AddLine(D2D1::Point2F(xOffset, yOffset + metrics.height));
            sink->EndFigure(D2D1_FIGURE_END_CLOSED);
            ThrowIfFailed(sink->Close(), "Geometry sink close failed");

            if (i == activeTextIndex_) {
                renderTarget_->FillGeometry(geometry.Get(), redBrush_.Get());
            } else {
                renderTarget_->DrawGeometry(geometry.Get(), redBrush_.Get(), 2.0f);
            }

            xOffset += triSize;
        }

        yOffset += maxHeight + border;

        auto sizeRT = renderTarget_->GetSize();
        float leftoverHeight = sizeRT.height - yOffset - 2 * border;
        float leftoverWidth = sizeRT.width - 2 * border;

        if (leftoverHeight > 0 && leftoverWidth > 0) {
            ComPtr<IDWriteTextLayout> smallLayout;
            ThrowIfFailed(
                writeFactory_->CreateTextLayout(
                    texts_[activeTextIndex_].c_str(),
                    static_cast<UINT32>(texts_[activeTextIndex_].size()),
                    textFormat_.Get(), // small font
                    leftoverWidth,
                    leftoverHeight,
                    &smallLayout
                ),
                "Failed to create small text layout"
            );

            if (smallLayout) {
                DWRITE_TEXT_METRICS smMetrics{};
                smallLayout->GetMetrics(&smMetrics);

                int cols = static_cast<int>(leftoverWidth / smMetrics.width);
                int rows = static_cast<int>(leftoverHeight / smMetrics.height);

                float totalWidth = cols * smMetrics.width;
                float totalHeight = rows * smMetrics.height;

                float offsetX = (leftoverWidth - totalWidth) * 0.5f + border;
                float offsetY2 = yOffset + border + (leftoverHeight - totalHeight) * 0.5f;

                brush_->SetColor(D2D1::ColorF(1, 1, 1, 1));
                for (int r = 0; r < rows; ++r) {
                    for (int c = 0; c < cols; ++c) {
                        float x = offsetX + c * smMetrics.width;
                        float y = offsetY2 + r * smMetrics.height;
                        renderTarget_->DrawTextLayout(
                            D2D1::Point2F(x, y),
                            smallLayout.Get(),
                            brush_.Get());
                    }
                }
            }
        }

        renderTarget_->EndDraw();
    }

    void OnChar(WPARAM wParam) {
        wchar_t ch = static_cast<wchar_t>(wParam);
        if (ch == 8) {
            if (!texts_.empty()) {
                auto &activeString = texts_[activeTextIndex_];
                if (!activeString.empty()) {
                    activeString.pop_back();
                    if (activeString.empty() && texts_.size() > 1) {
                        texts_.erase(texts_.begin() + activeTextIndex_);
                        if (activeTextIndex_ >= texts_.size()) {
                            activeTextIndex_ = texts_.size() - 1;
                        }
                    }
                    UpdateTextLayouts();
                    InvalidateRect(hwnd_, nullptr, TRUE);
                }
            }
        } else if (ch >= 0x20) {
            texts_[activeTextIndex_].push_back(ch);
            UpdateTextLayouts();
            InvalidateRect(hwnd_, nullptr, TRUE);
        }
    }

    void OnSize() {
        UpdateTextLayouts();
    }

    void OnPaint() {
        Draw();
    }

    bool OnKeyDown(WPARAM wParam) {
        if (wParam == VK_ESCAPE) {
            PostQuitMessage(0);
            return true;
        }
        switch (wParam) {
            case VK_TAB:
                if (GetKeyState(VK_SHIFT) & 0x8000) {
                    PrevText();
                } else {
                    NextText();
                }
                UpdateTextLayouts();
                InvalidateRect(hwnd_, nullptr, TRUE);
                return false;
            case VK_LEFT:
                PrevText();
                UpdateTextLayouts();
                InvalidateRect(hwnd_, nullptr, TRUE);
                return false;
            case VK_RIGHT:
                NextText();
                UpdateTextLayouts();
                InvalidateRect(hwnd_, nullptr, TRUE);
                return false;
            case VK_RETURN:
                std::cout << "RETURN" << std::endl;
                return true;
        }
        return false;
    }

    void OnDestroy() {
        PostQuitMessage(0);
    }

    void NextText() {
        if (texts_[activeTextIndex_].empty()) {
            if (texts_.size() > 1) {
                texts_.erase(texts_.begin() + activeTextIndex_);
                if (activeTextIndex_ >= texts_.size()) {
                    activeTextIndex_ = texts_.size() - 1;
                }
            }
        } else {
            texts_.insert(texts_.begin() + activeTextIndex_ + 1, L"");
            activeTextIndex_++;
        }
    }

    void PrevText() {
        if (texts_[activeTextIndex_].empty()) {
            if (texts_.size() > 1) {
                texts_.erase(texts_.begin() + activeTextIndex_);
                if (activeTextIndex_ > 0) {
                    activeTextIndex_--;
                }
                if (activeTextIndex_ >= texts_.size()) {
                    activeTextIndex_ = texts_.size() - 1;
                }
            }
        } else {
            texts_.insert(texts_.begin() + activeTextIndex_, L"");
        }
    }

private:
    ComPtr<ID2D1Factory> d2dFactory_;
    ComPtr<IDWriteFactory> writeFactory_;
    ComPtr<ID2D1HwndRenderTarget> renderTarget_;
    ComPtr<IDWriteTextFormat> textFormat_;
    ComPtr<IDWriteTextFormat> bigTextFormat_;
    ComPtr<ID2D1SolidColorBrush> brush_;
    ComPtr<ID2D1SolidColorBrush> redBrush_;
    ComPtr<ID2D1SolidColorBrush> brushGray_;
    std::vector<std::wstring> texts_;
    size_t activeTextIndex_ = 0;
    std::vector<ComPtr<IDWriteTextLayout> > layoutsBig_;
    HWND hwnd_ = nullptr;
};

// wWinMain: main entry point for Windows apps
int WINAPI wWinMain(HINSTANCE hInstance, HINSTANCE, PWSTR, int) {
    try {
        ComInit comInit;

        App myApp(hInstance);

        MSG msg{};
        while (GetMessageW(&msg, nullptr, 0, 0) > 0) {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        return static_cast<int>(msg.wParam);
    } catch (const std::exception &ex) {
        const char *what_utf8 = ex.what();
        std::wstring what_wide(what_utf8, what_utf8 + std::strlen(what_utf8));

        MessageBoxW(nullptr, what_wide.c_str(), L"Error", MB_OK);
        return -1;
    }
}

static LRESULT CALLBACK WindowProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    if (msg == WM_CREATE) {
        auto cs = reinterpret_cast<CREATESTRUCTW *>(lParam);
        App *app = reinterpret_cast<App *>(cs->lpCreateParams);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (LONG_PTR) app);
        return 0;
    }

    auto *app = reinterpret_cast<App *>(GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    if (!app) {
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }

    switch (msg) {
        case WM_PAINT: {
            PAINTSTRUCT ps;
            BeginPaint(hwnd, &ps);
            app->OnPaint();
            EndPaint(hwnd, &ps);
            return 0;
        }
        case WM_CHAR:
            app->OnChar(wParam);
            return 0;
        case WM_KEYDOWN:
            return app->OnKeyDown(wParam) ? 0 : 1;
        case WM_SIZE:
            app->OnSize();
            return 0;
        case WM_DESTROY:
            app->OnDestroy();
            return 0;
    }

    return DefWindowProcW(hwnd, msg, wParam, lParam);
}
