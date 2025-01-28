const std = @import("std");
const win32 = @import("win32").everything;
const c = std.zig.c;

fn L(comptime str: []const u8) [*:0]const u16 {
    return std.unicode.utf8ToUtf16LeStringLiteral(str);
}

// Global Direct2D/DirectWrite objects for brevity.
var g_factory: ?*win32.ID2D1Factory = null;
var g_renderTarget: ?*win32.ID2D1HwndRenderTarget = null;
var g_brush: ?*win32.ID2D1SolidColorBrush = null;
var g_writeFactory: ?*win32.IDWriteFactory = null;
var g_textFormat: ?*win32.IDWriteTextFormat = null;
var g_text_buffer: [256:0]u16 = undefined;
var g_text_len: usize = 0;

const CLASS_NAME = L("YasWindowClass");
const WINDOW_NAME = L("Hello world");

fn getTime() f32 {
    var counter: win32.LARGE_INTEGER = undefined;
    _ = win32.QueryPerformanceCounter(&counter);
    return @as(f32, @floatFromInt(counter.QuadPart)) / 10_000_000.0;
}

fn WindowProc(hWnd: win32.HWND, msg: c_uint, wParam: win32.WPARAM, lParam: win32.LPARAM) callconv(std.os.windows.WINAPI) win32.LRESULT {
    switch (msg) {
        win32.WM_DESTROY => {
            win32.PostQuitMessage(0);
            return 0;
        },
        win32.WM_PAINT => {
            var ps: win32.PAINTSTRUCT = undefined;
            _ = win32.BeginPaint(hWnd, &ps);

            if (g_renderTarget == null) {
                var rc: win32.RECT = .{ .left = 0, .right = 0, .top = 0, .bottom = 0 };
                _ = win32.GetClientRect(hWnd, &rc);

                const size: win32.D2D_SIZE_U = .{
                    .width = @intCast(rc.right - rc.left),
                    .height = @intCast(rc.bottom - rc.top),
                };

                // Create the HwndRenderTarget
                var targetProps: win32.D2D1_RENDER_TARGET_PROPERTIES = .{
                    .type = win32.D2D1_RENDER_TARGET_TYPE_DEFAULT,
                    .pixelFormat = .{ .format = win32.DXGI_FORMAT_UNKNOWN, .alphaMode = win32.D2D1_ALPHA_MODE_UNKNOWN },
                    .dpiX = 0,
                    .dpiY = 0,
                    .usage = win32.D2D1_RENDER_TARGET_USAGE_NONE,
                    .minLevel = win32.D2D1_FEATURE_LEVEL_DEFAULT,
                };
                var hwndProps: win32.D2D1_HWND_RENDER_TARGET_PROPERTIES = .{
                    .hwnd = hWnd,
                    .pixelSize = size,
                    .presentOptions = win32.D2D1_PRESENT_OPTIONS_NONE,
                };

                var rtOut: ?*win32.ID2D1HwndRenderTarget = null;
                _ = g_factory.?.CreateHwndRenderTarget(&targetProps, &hwndProps, @ptrCast(&rtOut));
                g_renderTarget = rtOut;

                var brushOut: ?*win32.ID2D1SolidColorBrush = null;
                _ = g_renderTarget.?.ID2D1RenderTarget.CreateSolidColorBrush(&.{ .r = 1, .g = 1, .b = 1, .a = 1 }, null, @ptrCast(&brushOut));
                g_brush = brushOut;
            }

            g_renderTarget.?.ID2D1RenderTarget.BeginDraw();
            g_renderTarget.?.ID2D1RenderTarget.Clear(&win32.D2D_COLOR_F{ .r = 0, .g = 0, .b = 0, .a = 1 });

            if (g_textFormat == null) {
                var tfOut: ?*win32.IDWriteTextFormat = null;
                _ = g_writeFactory.?.CreateTextFormat(L("Consolas"), null, win32.DWRITE_FONT_WEIGHT_REGULAR, win32.DWRITE_FONT_STYLE_NORMAL, win32.DWRITE_FONT_STRETCH_NORMAL, 48.0, L("en-us"), @ptrCast(&tfOut));
                g_textFormat = tfOut;

                // Left-align text for tiling
                _ = g_textFormat.?.SetTextAlignment(win32.DWRITE_TEXT_ALIGNMENT_LEADING);
                _ = g_textFormat.?.SetParagraphAlignment(win32.DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
            }

            // Get client size
            var clientRect: win32.RECT = .{ .left = 0, .right = 0, .top = 0, .bottom = 0 };
            _ = win32.GetClientRect(hWnd, &clientRect);
            const clientWidth: f32 = @floatFromInt(clientRect.right - clientRect.left);
            const clientHeight: f32 = @floatFromInt(clientRect.bottom - clientRect.top);

            // Replace the hardcoded text with the buffer
            const text = @as([*:0]const u16, &g_text_buffer);
            const text_len: u32 = @intCast(g_text_len);
            var textMetrics: win32.DWRITE_TEXT_METRICS = undefined;
            var layout: ?*win32.IDWriteTextLayout = null;

            _ = g_writeFactory.?.CreateTextLayout(
                text,
                text_len,
                g_textFormat,
                clientWidth, // max width
                clientHeight, // max height
                @ptrCast(&layout),
            );
            _ = layout.?.GetMetrics(&textMetrics);

            // Calculate grid
            const textWidth = textMetrics.widthIncludingTrailingWhitespace;
            const textHeight = textMetrics.height;
            const rows = @divTrunc(clientHeight, textHeight);
            const cols = @divTrunc(clientWidth, textWidth);

            // Draw text grid with cursors
            const current_time = getTime();
            const pulse = (1.0 + @sin(current_time * 4.0)) * 0.5; // Pulse between 0 and 1 every ~0.8 seconds

            // Create cursor brush once
            var cursorBrushOut: ?*win32.ID2D1SolidColorBrush = null;
            _ = g_renderTarget.?.ID2D1RenderTarget.CreateSolidColorBrush(
                &.{ .r = 1, .g = 1, .b = 1, .a = pulse },
                null,
                @ptrCast(&cursorBrushOut),
            );

            var y: f32 = (clientHeight - (textHeight * rows)) / 2;
            while (y + textHeight < clientHeight) : (y += textHeight) {
                var x: f32 = (clientWidth - (textWidth * cols)) / 2;
                while (x + textWidth < clientWidth) : (x += textWidth) {
                    // Draw text
                    const layoutRect = win32.D2D_RECT_F{
                        .left = x,
                        .top = y,
                        .right = x + textWidth,
                        .bottom = y + textHeight,
                    };
                    g_renderTarget.?.ID2D1RenderTarget.DrawText(
                        text,
                        text_len,
                        g_textFormat,
                        &layoutRect,
                        @ptrCast(g_brush),
                        win32.D2D1_DRAW_TEXT_OPTIONS_NONE,
                        win32.DWRITE_MEASURING_MODE_NATURAL,
                    );

                    // Draw cursor
                    const cursorWidth = textHeight * 0.1;
                    const cursorRect = win32.D2D_RECT_F{
                        .left = x + textWidth - cursorWidth,
                        .top = y,
                        .right = x + textWidth,
                        .bottom = y + textHeight,
                    };
                    g_renderTarget.?.ID2D1RenderTarget.FillRectangle(&cursorRect, @ptrCast(cursorBrushOut));
                }
            }

            // Release cursor brush
            _ = cursorBrushOut.?.IUnknown.Release();

            _ = layout.?.IUnknown.Release();

            _ = g_renderTarget.?.ID2D1RenderTarget.EndDraw(null, null);

            // Force continuous redraw for animation
            _ = win32.InvalidateRect(hWnd, null, 0);

            _ = win32.EndPaint(hWnd, &ps);
            return 0;
        },
        win32.WM_CHAR => {
            const char = @as(u16, @intCast(wParam));
            switch (char) {
                8 => { // Backspace
                    if (g_text_len > 0) {
                        g_text_len -= 1;
                        g_text_buffer[g_text_len] = 0; // null terminate
                        _ = win32.InvalidateRect(hWnd, null, 1);
                    }
                },
                0x0A, 0x0D => {}, // Ignore CR/LF
                0...7, 9, 0x0B, 0x0C, 0x0E...0x1F => {}, // Ignore other control characters
                else => { // Accept all other Unicode characters
                    if (g_text_len < g_text_buffer.len - 1) { // Leave room for null terminator
                        g_text_buffer[g_text_len] = char;
                        g_text_len += 1;
                        g_text_buffer[g_text_len] = 0; // null terminate
                        _ = win32.InvalidateRect(hWnd, null, 1);
                    }
                },
            }
            return 0;
        },
        win32.WM_KEYDOWN => {
            if (wParam == @intFromEnum(win32.VK_ESCAPE)) {
                win32.PostQuitMessage(0);
                return 0;
            }
            return win32.DefWindowProcW(hWnd, msg, wParam, lParam);
        },
        else => return win32.DefWindowProcW(hWnd, msg, wParam, lParam),
    }
}

pub fn main() !void {
    _ = win32.CoInitializeEx(null, win32.COINIT_APARTMENTTHREADED);

    // Initialize text buffer with "Hello world"
    const initial_text = L("Hello");
    const initial_len = std.mem.len(initial_text);
    std.mem.copy(u16, g_text_buffer, initial_text[0..initial_len]);
    g_text_len = initial_len;
    g_text_buffer[g_text_len] = 0; // null-terminate

    var factoryOut: ?*win32.ID2D1Factory = null;
    _ = win32.D2D1CreateFactory(win32.D2D1_FACTORY_TYPE_SINGLE_THREADED, win32.IID_ID2D1Factory, null, @ptrCast(&factoryOut));
    g_factory = factoryOut;

    var dwriteFactoryOut: ?*win32.IDWriteFactory = null;
    _ = win32.DWriteCreateFactory(win32.DWRITE_FACTORY_TYPE_SHARED, win32.IID_IDWriteFactory, @ptrCast(&dwriteFactoryOut));
    g_writeFactory = dwriteFactoryOut;

    // Get screen size for fullscreen
    const hMonitor = win32.MonitorFromPoint(win32.POINT{ .x = 0, .y = 0 }, win32.MONITOR_DEFAULTTOPRIMARY);
    // create a zeroed temporary struct
    var mi: win32.MONITORINFO = @bitCast([_]u8{0} ** (@sizeOf(win32.MONITORINFO)));
    mi.cbSize = @sizeOf(win32.MONITORINFO);
    _ = win32.GetMonitorInfoW(hMonitor, &mi);

    const width = mi.rcMonitor.right - mi.rcMonitor.left;
    const height = mi.rcMonitor.bottom - mi.rcMonitor.top;

    // Register window class
    var wcx: win32.WNDCLASSEXW = .{
        .cbSize = @sizeOf(win32.WNDCLASSEXW),
        .style = win32.WNDCLASS_STYLES{},
        .lpfnWndProc = WindowProc,
        .hInstance = win32.GetModuleHandleW(null),
        .hCursor = win32.LoadCursorW(null, win32.IDC_ARROW),
        .hbrBackground = null,
        .lpszClassName = @ptrCast(&CLASS_NAME[0]),
        .cbClsExtra = 0,
        .cbWndExtra = 0,
        .hIcon = null,
        .lpszMenuName = null,
        .hIconSm = null,
    };
    _ = win32.RegisterClassExW(&wcx);
    const hwnd = win32.CreateWindowExW(win32.WINDOW_EX_STYLE{}, @ptrCast(&CLASS_NAME[0]), @ptrCast(&WINDOW_NAME[0]), win32.WINDOW_STYLE{ .POPUP = 1, .VISIBLE = 1 }, mi.rcMonitor.left, mi.rcMonitor.top, width, height, null, null, wcx.hInstance, null);
    _ = win32.ShowWindow(hwnd, win32.SW_SHOW);
    _ = win32.UpdateWindow(hwnd);

    var msg: win32.MSG = undefined;
    while (win32.GetMessageW(&msg, null, 0, 0) != 0) {
        _ = win32.TranslateMessage(&msg);
        _ = win32.DispatchMessageW(&msg);
    }
    win32.CoUninitialize();
}
