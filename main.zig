const std = @import("std");
const win32 = @import("win32").everything;
const c = std.zig.c;

fn L(comptime str: []const u8) [*:0]const u16 {
    return std.unicode.utf8ToUtf16LeStringLiteral(str);
}

const CLASS_NAME = L("YasWindowClass");
const WINDOW_NAME = L("Hello world");

const App = struct {
    factory: ?*win32.ID2D1Factory = null,
    renderTarget: ?*win32.ID2D1HwndRenderTarget = null,
    brush: ?*win32.ID2D1SolidColorBrush = null,
    writeFactory: ?*win32.IDWriteFactory = null,
    textFormat: ?*win32.IDWriteTextFormat = null,
    bigTextFormat: ?*win32.IDWriteTextFormat = null,
    textBuffer: [256:0]u16 = undefined,
    textLen: usize = 0,
    hwnd: ?win32.HWND = null,

    pub fn init() !App {
        var app = App{};

        // Initialize COM
        _ = win32.CoInitializeEx(null, win32.COINIT_APARTMENTTHREADED);
        errdefer win32.CoUninitialize();

        // Initialize text buffer with "Hello"
        const initial_text = L("Hello");
        const initial_len = std.mem.len(initial_text);
        @memcpy(app.textBuffer[0..initial_len], initial_text[0..initial_len]);
        app.textLen = initial_len;
        app.textBuffer[app.textLen] = 0; // null-terminate

        // Create Direct2D factory
        var factoryOut: ?*win32.ID2D1Factory = null;
        const hr1 = win32.D2D1CreateFactory(win32.D2D1_FACTORY_TYPE_SINGLE_THREADED, win32.IID_ID2D1Factory, null, @ptrCast(&factoryOut));
        if (hr1 != win32.S_OK) return error.D2DFactoryCreateFailed;
        app.factory = factoryOut;

        // Create DirectWrite factory
        var dwriteFactoryOut: ?*win32.IDWriteFactory = null;
        const hr2 = win32.DWriteCreateFactory(win32.DWRITE_FACTORY_TYPE_SHARED, win32.IID_IDWriteFactory, @ptrCast(&dwriteFactoryOut));
        if (hr2 != win32.S_OK) return error.DWriteFactoryCreateFailed;
        app.writeFactory = dwriteFactoryOut;

        return app;
    }

    pub fn deinit(self: *App) void {
        if (self.brush != null) {
            _ = self.brush.?.IUnknown.Release();
            self.brush = null;
        }
        if (self.renderTarget != null) {
            _ = self.renderTarget.?.IUnknown.Release();
            self.renderTarget = null;
        }
        if (self.textFormat != null) {
            _ = self.textFormat.?.IUnknown.Release();
            self.textFormat = null;
        }
        if (self.bigTextFormat != null) {
            _ = self.bigTextFormat.?.IUnknown.Release();
            self.bigTextFormat = null;
        }
        if (self.writeFactory != null) {
            _ = self.writeFactory.?.IUnknown.Release();
            self.writeFactory = null;
        }
        if (self.factory != null) {
            _ = self.factory.?.IUnknown.Release();
            self.factory = null;
        }
        win32.CoUninitialize();
    }

    fn windowProcThunk(hwnd: win32.HWND, msg: c_uint, wp: win32.WPARAM, lp: win32.LPARAM) callconv(std.os.windows.WINAPI) win32.LRESULT {
        if (msg == win32.WM_CREATE) {
            const create_struct = @as(*win32.CREATESTRUCTW, @ptrFromInt(@as(u64, @intCast(lp))));
            const self = @as(*App, @ptrCast(@alignCast(create_struct.lpCreateParams)));
            _ = win32.SetWindowLongPtrW(hwnd, win32.GWLP_USERDATA, @intCast(@as(usize, @intFromPtr(self))));
            self.hwnd = hwnd;
        }

        const ptr = win32.GetWindowLongPtrW(hwnd, win32.GWLP_USERDATA);
        const self = if (ptr != 0) @as(*App, @ptrFromInt(@as(u64, @intCast(ptr)))) else null;

        if (self) |app| {
            return app.windowProc(hwnd, msg, wp, lp);
        }
        return win32.DefWindowProcW(hwnd, msg, wp, lp);
    }

    fn windowProc(self: *App, hWnd: win32.HWND, msg: c_uint, wParam: win32.WPARAM, lParam: win32.LPARAM) win32.LRESULT {
        switch (msg) {
            win32.WM_DESTROY => {
                win32.PostQuitMessage(0);
                return 0;
            },
            win32.WM_PAINT => {
                return self.onPaint();
            },
            win32.WM_CHAR => {
                return self.onChar(wParam);
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

    fn onChar(self: *App, wParam: win32.WPARAM) win32.LRESULT {
        const char = @as(u16, @intCast(wParam));
        switch (char) {
            8 => { // Backspace
                if (self.textLen > 0) {
                    self.textLen -= 1;
                    self.textBuffer[self.textLen] = 0; // null terminate
                    _ = win32.InvalidateRect(self.hwnd, null, 1);
                }
            },
            0...7, 9, 0x0B, 0x0C, 0x0E...0x1F => {}, // Ignore other control characters
            else => { // Accept all other Unicode characters
                if (self.textLen < self.textBuffer.len - 1) { // Leave room for null terminator
                    self.textBuffer[self.textLen] = char;
                    self.textLen += 1;
                    self.textBuffer[self.textLen] = 0; // null terminate
                    _ = win32.InvalidateRect(self.hwnd, null, 1);
                }
            },
        }
        return 0;
    }

    fn onPaint(self: *App) win32.LRESULT {
        var ps: win32.PAINTSTRUCT = undefined;
        _ = win32.BeginPaint(self.hwnd, &ps);
        defer _ = win32.EndPaint(self.hwnd, &ps);

        self.ensureRenderTarget();
        self.draw();

        return 0;
    }

    fn ensureRenderTarget(self: *App) void {
        if (self.renderTarget != null) return;

        var rc: win32.RECT = .{ .left = 0, .right = 0, .top = 0, .bottom = 0 };
        _ = win32.GetClientRect(self.hwnd, &rc);

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
            .hwnd = self.hwnd,
            .pixelSize = size,
            .presentOptions = win32.D2D1_PRESENT_OPTIONS_NONE,
        };

        var rtOut: ?*win32.ID2D1HwndRenderTarget = null;
        _ = self.factory.?.CreateHwndRenderTarget(&targetProps, &hwndProps, @ptrCast(&rtOut));
        self.renderTarget = rtOut;

        var brushOut: ?*win32.ID2D1SolidColorBrush = null;
        _ = self.renderTarget.?.ID2D1RenderTarget.CreateSolidColorBrush(&.{ .r = 1, .g = 1, .b = 1, .a = 1 }, null, @ptrCast(&brushOut));
        self.brush = brushOut;
    }

    fn ensureTextFormats(self: *App) void {
        if (self.textFormat == null) {
            var tfOut: ?*win32.IDWriteTextFormat = null;
            _ = self.writeFactory.?.CreateTextFormat(
                L("Consolas"),
                null,
                win32.DWRITE_FONT_WEIGHT_REGULAR,
                win32.DWRITE_FONT_STYLE_NORMAL,
                win32.DWRITE_FONT_STRETCH_NORMAL,
                32.0,
                L("en-us"),
                @ptrCast(&tfOut),
            );
            self.textFormat = tfOut;
            _ = self.textFormat.?.SetTextAlignment(win32.DWRITE_TEXT_ALIGNMENT_CENTER);
            _ = self.textFormat.?.SetParagraphAlignment(win32.DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        }

        if (self.bigTextFormat == null) {
            var tfBigOut: ?*win32.IDWriteTextFormat = null;
            _ = self.writeFactory.?.CreateTextFormat(
                L("Consolas"),
                null,
                win32.DWRITE_FONT_WEIGHT_BOLD,
                win32.DWRITE_FONT_STYLE_NORMAL,
                win32.DWRITE_FONT_STRETCH_NORMAL,
                64.0,
                L("en-us"),
                @ptrCast(&tfBigOut),
            );
            self.bigTextFormat = tfBigOut;
            _ = self.bigTextFormat.?.SetTextAlignment(win32.DWRITE_TEXT_ALIGNMENT_CENTER);
            _ = self.bigTextFormat.?.SetParagraphAlignment(win32.DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        }
    }

    fn draw(self: *App) void {
        self.ensureTextFormats();

        self.renderTarget.?.ID2D1RenderTarget.BeginDraw();
        defer _ = self.renderTarget.?.ID2D1RenderTarget.EndDraw(null, null);

        self.renderTarget.?.ID2D1RenderTarget.Clear(&win32.D2D_COLOR_F{ .r = 0, .g = 0, .b = 0, .a = 1 });

        // Get client size
        var clientRect: win32.RECT = .{ .left = 0, .right = 0, .top = 0, .bottom = 0 };
        _ = win32.GetClientRect(self.hwnd, &clientRect);
        const clientWidth: f32 = @floatFromInt(clientRect.right - clientRect.left);
        const clientHeight: f32 = @floatFromInt(clientRect.bottom - clientRect.top);

        // Default metrics for empty text
        var bigTextWidth: f32 = 0;
        var bigTextHeight: f32 = 64.0;
        var smallTextWidth: f32 = 0;
        var smallTextHeight: f32 = 32.0;

        const text_ptr = @as([*:0]const u16, &self.textBuffer);
        const text_len: u32 = @intCast(self.textLen);

        if (text_len > 0) {
            // Get text metrics
            var layoutBig: ?*win32.IDWriteTextLayout = null;
            _ = self.writeFactory.?.CreateTextLayout(
                text_ptr,
                text_len,
                self.bigTextFormat,
                clientWidth,
                clientHeight,
                @ptrCast(&layoutBig),
            );
            defer _ = layoutBig.?.IUnknown.Release();

            var bigMetrics: win32.DWRITE_TEXT_METRICS = undefined;
            _ = layoutBig.?.GetMetrics(&bigMetrics);
            bigTextWidth = bigMetrics.widthIncludingTrailingWhitespace;
            bigTextHeight = bigMetrics.height;

            var layoutSmall: ?*win32.IDWriteTextLayout = null;
            _ = self.writeFactory.?.CreateTextLayout(
                text_ptr,
                text_len,
                self.textFormat,
                clientWidth,
                clientHeight,
                @ptrCast(&layoutSmall),
            );
            defer _ = layoutSmall.?.IUnknown.Release();

            var smallMetrics: win32.DWRITE_TEXT_METRICS = undefined;
            _ = layoutSmall.?.GetMetrics(&smallMetrics);
            smallTextWidth = smallMetrics.widthIncludingTrailingWhitespace;
            smallTextHeight = smallMetrics.height;
        }

        // Draw cursor
        var cursorBrushOut: ?*win32.ID2D1SolidColorBrush = null;
        _ = self.renderTarget.?.ID2D1RenderTarget.CreateSolidColorBrush(
            &.{ .r = 1, .g = 1, .b = 1, .a = 1.0 },
            null,
            @ptrCast(&cursorBrushOut),
        );
        defer _ = cursorBrushOut.?.IUnknown.Release();

        const cursorWidth = 12.0;
        const cursorRect = win32.D2D_RECT_F{
            .left = (clientWidth - cursorWidth) / 2,
            .top = 0,
            .right = (clientWidth + cursorWidth) / 2,
            .bottom = bigTextHeight,
        };
        self.renderTarget.?.ID2D1RenderTarget.FillRectangle(&cursorRect, @ptrCast(cursorBrushOut));

        if (text_len > 0) {
            // Draw big text
            const x: f32 = (clientWidth - bigTextWidth) / 2;
            const layoutRect = win32.D2D_RECT_F{
                .left = x,
                .top = 0,
                .right = x + bigTextWidth,
                .bottom = bigTextHeight,
            };
            self.renderTarget.?.ID2D1RenderTarget.DrawText(
                text_ptr,
                text_len,
                self.bigTextFormat,
                &layoutRect,
                @ptrCast(self.brush),
                win32.D2D1_DRAW_TEXT_OPTIONS_NONE,
                win32.DWRITE_MEASURING_MODE_NATURAL,
            );

            // Draw small text copies
            var y: f32 = bigTextHeight;
            const smallRows = @divTrunc(clientHeight - y, smallTextHeight);
            var row_index: f32 = 0;
            while (row_index < smallRows) : (row_index += 1) {
                const cols = @divTrunc(clientWidth, smallTextWidth);
                var x2 = (clientWidth - (cols * smallTextWidth)) / 2;

                var col_index: f32 = 0;
                while (col_index < cols) : (col_index += 1) {
                    const layoutRectSmall = win32.D2D_RECT_F{
                        .left = x2,
                        .top = y,
                        .right = x2 + smallTextWidth,
                        .bottom = y + smallTextHeight,
                    };

                    self.renderTarget.?.ID2D1RenderTarget.DrawText(
                        text_ptr,
                        text_len,
                        self.textFormat,
                        &layoutRectSmall,
                        @ptrCast(self.brush),
                        win32.D2D1_DRAW_TEXT_OPTIONS_NONE,
                        win32.DWRITE_MEASURING_MODE_NATURAL,
                    );

                    x2 += smallTextWidth;
                }
                y += smallTextHeight;
            }
        }
    }

    pub fn run(self: *App) !void {
        // Get screen size for fullscreen
        const hMonitor = win32.MonitorFromPoint(win32.POINT{ .x = 0, .y = 0 }, win32.MONITOR_DEFAULTTOPRIMARY);
        var mi: win32.MONITORINFO = @bitCast([_]u8{0} ** (@sizeOf(win32.MONITORINFO)));
        mi.cbSize = @sizeOf(win32.MONITORINFO);
        _ = win32.GetMonitorInfoW(hMonitor, &mi);

        const width = mi.rcMonitor.right - mi.rcMonitor.left;
        const height = mi.rcMonitor.bottom - mi.rcMonitor.top;

        // Register window class
        var wcx = win32.WNDCLASSEXW{
            .cbSize = @sizeOf(win32.WNDCLASSEXW),
            .style = win32.WNDCLASS_STYLES{},
            .lpfnWndProc = App.windowProcThunk,
            .hInstance = win32.GetModuleHandleW(null),
            .hCursor = win32.LoadCursorW(null, win32.IDC_ARROW),
            .hbrBackground = null,
            .lpszClassName = CLASS_NAME,
            .cbClsExtra = 0,
            .cbWndExtra = 0,
            .hIcon = null,
            .lpszMenuName = null,
            .hIconSm = null,
        };

        _ = win32.RegisterClassExW(&wcx);

        // Create window
        const hwnd = win32.CreateWindowExW(
            win32.WINDOW_EX_STYLE{},
            CLASS_NAME,
            WINDOW_NAME,
            win32.WINDOW_STYLE{ .POPUP = 1, .VISIBLE = 1 },
            mi.rcMonitor.left,
            mi.rcMonitor.top,
            width,
            height,
            null,
            null,
            wcx.hInstance,
            self,
        );

        if (hwnd == null) return error.WindowCreationFailed;

        _ = win32.ShowWindow(hwnd, win32.SW_SHOW);
        _ = win32.UpdateWindow(hwnd);

        // Message loop
        var msg: win32.MSG = undefined;
        while (win32.GetMessageW(&msg, null, 0, 0) != 0) {
            _ = win32.TranslateMessage(&msg);
            _ = win32.DispatchMessageW(&msg);
        }
    }
};

pub fn main() !void {
    var app = try App.init();
    defer app.deinit();
    try app.run();
}
