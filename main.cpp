#include <windows.h>
#include <d2d1.h>
#include <dwrite.h>
#include <string>
#include <stdexcept>
#include <wrl/client.h>

using Microsoft::WRL::ComPtr;

// RAII helper for COM initialization
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

// Forward declaration of our Win32 window procedure
static LRESULT CALLBACK WindowProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam);

class App {
public:
    App(HINSTANCE hInstance) {
        // Initialize COM via RAII
        // (We keep a member ComInit object or create one on the stack in wWinMain.)
        // In this example, we'll do it externally in wWinMain so the entire app has COM.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        text_ = WINDOW_NAME; // initial text

        // Create Direct2D factory
        HRESULT hr = D2D1CreateFactory(
            D2D1_FACTORY_TYPE_SINGLE_THREADED,
            d2dFactory_.GetAddressOf()
        );
        if (FAILED(hr) || !d2dFactory_) {
            throw std::runtime_error("Failed to create Direct2D factory");
        }

        // Create DirectWrite factory
        hr = DWriteCreateFactory(
            DWRITE_FACTORY_TYPE_SHARED,
            __uuidof(IDWriteFactory),
            reinterpret_cast<IUnknown **>(writeFactory_.GetAddressOf())
        );
        if (FAILED(hr) || !writeFactory_) {
            throw std::runtime_error("Failed to create DirectWrite factory");
        }

        // Retrieve user locale. If it fails, fall back to 'en-us'.
        wchar_t localeName[LOCALE_NAME_MAX_LENGTH] = {};
        if (0 == GetUserDefaultLocaleName(localeName, LOCALE_NAME_MAX_LENGTH)) {
            wcscpy_s(localeName, L"en-us");
        }

        // Create text formats (small and big)
        hr = writeFactory_->CreateTextFormat(
            WINDOW_NAME,
            nullptr,
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            16.0f,
            localeName,
            textFormat_.GetAddressOf()
        );
        if (FAILED(hr) || !textFormat_) {
            throw std::runtime_error("Failed to create small textFormat");
        }
        hr = writeFactory_->CreateTextFormat(
            WINDOW_NAME,
            nullptr,
            DWRITE_FONT_WEIGHT_BOLD,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            32.0f,
            localeName,
            bigTextFormat_.GetAddressOf()
        );
        if (FAILED(hr) || !bigTextFormat_) {
            throw std::runtime_error("Failed to create big textFormat");
        }

        // Register the window class
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

        // Step 1: get current mouse position
        POINT pt;
        GetCursorPos(&pt);

        // Step 2: find the monitor from this point
        HMONITOR hMonitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST); // or MONITOR_DEFAULTTOPRIMARY
        MONITORINFO mi{};
        mi.cbSize = sizeof(mi);
        if (!GetMonitorInfoW(hMonitor, &mi)) {
            throw std::runtime_error("GetMonitorInfoW failed");
        }
        // Step 3: retrieve bounding rect
        int x = mi.rcMonitor.left;
        int y = mi.rcMonitor.top;
        int w = mi.rcMonitor.right - mi.rcMonitor.left;
        int h = mi.rcMonitor.bottom - mi.rcMonitor.top;

        // Step 4: create the window to fill that monitor
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
            this
        );

        if (!hwnd_) {
            throw std::runtime_error("CreateWindowExW failed");
        }

        // Create the render target
        RECT rc{};
        GetClientRect(hwnd_, &rc);
        D2D1_SIZE_U size = D2D1::SizeU(
            static_cast<UINT>(rc.right - rc.left),
            static_cast<UINT>(rc.bottom - rc.top)
        );
        hr = d2dFactory_->CreateHwndRenderTarget(
            D2D1::RenderTargetProperties(),
            D2D1::HwndRenderTargetProperties(hwnd_, size),
            &renderTarget_
        );
        if (FAILED(hr) || !renderTarget_) {
            throw std::runtime_error("Failed to create HwndRenderTarget");
        }

        // Create brushes
        hr = renderTarget_->CreateSolidColorBrush(
            D2D1::ColorF(1, 1, 1, 1), // white
            &brush_
        );
        if (FAILED(hr) || !brush_) {
            throw std::runtime_error("Failed to create white brush");
        }
        hr = renderTarget_->CreateSolidColorBrush(
            D2D1::ColorF(1, 0, 0, 1), // red
            &redBrush_
        );
        if (FAILED(hr) || !redBrush_) {
            throw std::runtime_error("Failed to create red brush");
        }

        ShowWindow(hwnd_, SW_SHOW);
        UpdateTextLayouts();
    }

    void UpdateTextLayouts() {
        if (!renderTarget_) {
            return;
        }

        layoutBig_.Reset();
        const UINT32 textLen = static_cast<UINT32>(text_.size());
        const wchar_t *textPtr = text_.c_str();
        auto sizeRT = renderTarget_->GetSize();

        // Create the big layout
        HRESULT hr = writeFactory_->CreateTextLayout(
            textPtr,
            textLen,
            bigTextFormat_.Get(),
            sizeRT.width,
            sizeRT.height,
            &layoutBig_
        );
        if (FAILED(hr)) {
            layoutBig_.Reset();
        }

        // Create the small layout + small-text bitmap
        layoutSmall_.Reset();
        smallTextBitmap_.Reset();
        if (textLen > 0) {
            hr = writeFactory_->CreateTextLayout(
                textPtr,
                textLen,
                textFormat_.Get(),
                sizeRT.width,
                sizeRT.height,
                &layoutSmall_
            );
            if (FAILED(hr)) {
                layoutSmall_.Reset();
                return;
            }

            // Retrieve metrics to compute width/height
            DWRITE_TEXT_METRICS smallMetrics{};
            if (SUCCEEDED(layoutSmall_->GetMetrics(&smallMetrics))) {
                smallTextWidth_ = smallMetrics.widthIncludingTrailingWhitespace;
                smallTextHeight_ = smallMetrics.height;

                if (smallTextWidth_ > 0.0f && smallTextHeight_ > 0.0f) {
                    auto size = D2D1::SizeF(smallTextWidth_, smallTextHeight_);
                    ComPtr<ID2D1BitmapRenderTarget> bitmapRT;
                    HRESULT hr2 = renderTarget_->CreateCompatibleRenderTarget(
                        &size,
                        nullptr,
                        nullptr,
                        D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
                        &bitmapRT
                    );
                    if (SUCCEEDED(hr2) && bitmapRT) {
                        ComPtr<ID2D1SolidColorBrush> tempBrush;
                        hr2 = bitmapRT->CreateSolidColorBrush(
                            D2D1::ColorF(1, 1, 1, 1),
                            &tempBrush
                        );
                        if (SUCCEEDED(hr2) && tempBrush) {
                            bitmapRT->BeginDraw();
                            bitmapRT->Clear(D2D1::ColorF(0, 0, 0, 0));
                            bitmapRT->DrawTextLayout(
                                D2D1::Point2F(0.0f, 0.0f),
                                layoutSmall_.Get(),
                                tempBrush.Get(),
                                D2D1_DRAW_TEXT_OPTIONS_NONE
                            );
                            bitmapRT->EndDraw();

                            ComPtr<ID2D1Bitmap> bmp;
                            if (SUCCEEDED(bitmapRT->GetBitmap(&bmp)) && bmp) {
                                smallTextBitmap_ = bmp;
                            }
                        }
                    }
                }
            }
        }
    }

    void Draw() {
        if (!renderTarget_) return;
        renderTarget_->BeginDraw();

        // Clear background
        renderTarget_->Clear(D2D1::ColorF(0, 0, 0, 1));

        // Draw big text
        if (layoutBig_) {
            renderTarget_->DrawTextLayout(
                D2D1::Point2F(0.0f, 0.0f),
                layoutBig_.Get(),
                brush_.Get(),
                D2D1_DRAW_TEXT_OPTIONS_NONE
            );

            // Attempt to place a cursor
            // We find the trailing edge unless text is empty
            UINT32 pos = text_.empty() ? 0 : static_cast<UINT32>(text_.size());
            DWRITE_HIT_TEST_METRICS hit{};
            float x = 0.0f, y = 0.0f;
            layoutBig_->HitTestTextPosition(pos, TRUE, &x, &y, &hit);

            float lineHeight = (hit.height > 0.0f) ? hit.height : 16.0f;
            float cursorHeight = lineHeight * 0.8f;
            float cursorX = x;
            float cursorY = y + (lineHeight - cursorHeight) / 2.0f;

            // Build a small triangle for the cursor
            ComPtr<ID2D1PathGeometry> geometry;
            if (SUCCEEDED(d2dFactory_->CreatePathGeometry(&geometry)) && geometry) {
                ComPtr<ID2D1GeometrySink> sink;
                if (SUCCEEDED(geometry->Open(&sink)) && sink) {
                    sink->BeginFigure(
                        D2D1::Point2F(cursorX, cursorY),
                        D2D1_FIGURE_BEGIN_FILLED
                    );
                    sink->AddLine(D2D1::Point2F(cursorX + 15.0f, cursorY + cursorHeight / 2.0f));
                    sink->AddLine(D2D1::Point2F(cursorX, cursorY + cursorHeight));
                    sink->EndFigure(D2D1_FIGURE_END_CLOSED);
                    sink->Close();
                }
                renderTarget_->FillGeometry(geometry.Get(), redBrush_.Get());
            }
        }

        // Draw repeated small text if not empty
        if (!text_.empty() && smallTextBitmap_) {
            D2D1_SIZE_F size = renderTarget_->GetSize();
            float cols = floorf(size.width / smallTextWidth_);
            float rows = floorf(size.height / smallTextHeight_);

            // Position grid under big text's bounding box
            DWRITE_TEXT_METRICS bigMetrics{};
            if (layoutBig_) {
                layoutBig_->GetMetrics(&bigMetrics);
            }
            float yGrid = bigMetrics.height; // simple offset from big text
            for (float row = 0; row < rows; row += 1.0f) {
                float xGrid = (size.width - (cols * smallTextWidth_)) / 2.0f;
                for (float col = 0; col < cols; col += 1.0f) {
                    renderTarget_->DrawBitmap(
                        smallTextBitmap_.Get(),
                        D2D1::RectF(
                            xGrid,
                            yGrid,
                            xGrid + smallTextWidth_,
                            yGrid + smallTextHeight_
                        ),
                        1.0f,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR
                    );
                    xGrid += smallTextWidth_;
                }
                yGrid += smallTextHeight_;
            }
        }

        renderTarget_->EndDraw();
    }

    // Called when the user types a character
    void OnChar(WPARAM wParam) {
        wchar_t ch = static_cast<wchar_t>(wParam);
        switch (ch) {
            case 8: // backspace
                if (!text_.empty()) {
                    text_.pop_back();
                    UpdateTextLayouts();
                    InvalidateRect(hwnd_, nullptr, TRUE);
                }
                break;
            default:
                text_.push_back(ch);
                UpdateTextLayouts();
                InvalidateRect(hwnd_, nullptr, TRUE);
        }
    }

    void OnSize() {
        UpdateTextLayouts();
    }

private:
    ComPtr<ID2D1Factory> d2dFactory_;
    ComPtr<IDWriteFactory> writeFactory_;
    ComPtr<ID2D1HwndRenderTarget> renderTarget_;
    ComPtr<IDWriteTextFormat> textFormat_;
    ComPtr<IDWriteTextFormat> bigTextFormat_;
    ComPtr<IDWriteTextLayout> layoutBig_;
    ComPtr<IDWriteTextLayout> layoutSmall_;
    ComPtr<ID2D1Bitmap> smallTextBitmap_;
    ComPtr<ID2D1SolidColorBrush> brush_;
    ComPtr<ID2D1SolidColorBrush> redBrush_;

    // Basic text data
    std::wstring text_;
    float smallTextWidth_ = 0.0f;
    float smallTextHeight_ = 0.0f;

    // Our window handle
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
            app->Draw();
            EndPaint(hwnd, &ps);
            return 0;
        }
        case WM_CHAR:
            app->OnChar(wParam);
            return 0;
        case WM_KEYDOWN:
            if (wParam == VK_ESCAPE) {
                PostQuitMessage(0);
                return 0;
            }
            break;
        case WM_SIZE: {
            app->OnSize();
            return 0;
        }
        case WM_DESTROY:
            PostQuitMessage(0);
            return 0;
    }

    return DefWindowProcW(hwnd, msg, wParam, lParam);
}
