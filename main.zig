const std = @import("std");
const win32 = @import("win32").everything;
const c = std.zig.c;

fn L(comptime str: []const u8) [*:0]const u16 {
    return std.unicode.utf8ToUtf16LeStringLiteral(str);
}

fn LS(comptime str: []const u8) []const u16 {
    return std.unicode.utf8ToUtf16LeStringLiteral(str);
}

const className = L("YasWindowClass");
const windowName = L("YAS!");

const App = struct {
    bigTextFormat: ?*win32.IDWriteTextFormat = null,
    brush: ?*win32.ID2D1SolidColorBrush = null,
    factory: ?*win32.ID2D1Factory = null,
    hwnd: ?win32.HWND = null,
    layoutBig: ?*win32.IDWriteTextLayout = null,
    layoutSmall: ?*win32.IDWriteTextLayout = null, // For small text
    localeBuffer: [win32.LOCALE_NAME_MAX_LENGTH:0]u16 = undefined,
    redBrush: ?*win32.ID2D1SolidColorBrush = null,
    renderTarget: ?*win32.ID2D1HwndRenderTarget = null,
    smallTextBitmap: ?*win32.ID2D1Bitmap = null,
    textFormat: ?*win32.IDWriteTextFormat = null,
    textLen: usize = 0,
    writeFactory: ?*win32.IDWriteFactory = null,
    lineHeight: f32 = 0,
    smallTextHeight: f32 = 0,
    smallTextWidth: f32 = 0,
    textBuffer: [1024:0]u16 = undefined,

    pub fn init() !App {
        var app = App{};

        // Initialize COM
        _ = win32.CoInitializeEx(null, win32.COINIT_APARTMENTTHREADED);
        errdefer win32.CoUninitialize();

        // Enable per-monitor DPI awareness V2
        _ = win32.SetProcessDpiAwarenessContext(win32.DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Initialize text buffer with "Hello"
        const initialText = windowName;
        const initialTextLen = std.mem.len(initialText);
        @memcpy(app.textBuffer[0..initialTextLen], initialText[0..initialTextLen]);
        app.textLen = initialTextLen;
        app.textBuffer[app.textLen] = 0; // null-terminate

        // Get the system's default locale name
        const res = win32.GetUserDefaultLocaleName(@ptrCast(&app.localeBuffer), app.localeBuffer.len);
        if (res == 0) {
            // Fallback to "en-us" if call fails
            const fallback = LS("en-us");
            @memcpy(app.localeBuffer[0..fallback.len], fallback[0..fallback.len]);
            app.localeBuffer[fallback.len] = 0;
        }

        // Create Direct2D factory
        var factoryOut: ?*win32.ID2D1Factory = null;
        const hr1 = win32.D2D1CreateFactory(win32.D2D1_FACTORY_TYPE_SINGLE_THREADED, win32.IID_ID2D1Factory, null, @ptrCast(&factoryOut));
        if (hr1 != win32.S_OK) return error.D2DFactoryCreateFailed;
        if (factoryOut == null) return error.D2DFactoryCreateFailed;
        app.factory = factoryOut;

        // Create DirectWrite factory
        var dwriteFactoryOut: ?*win32.IDWriteFactory = null;
        const hr2 = win32.DWriteCreateFactory(win32.DWRITE_FACTORY_TYPE_SHARED, win32.IID_IDWriteFactory, @ptrCast(&dwriteFactoryOut));
        if (hr2 != win32.S_OK) return error.DWriteFactoryCreateFailed;
        if (dwriteFactoryOut == null) return error.DWriteFactoryCreateFailed;
        app.writeFactory = dwriteFactoryOut;

        // Now create textFormat
        var tfOut: ?*win32.IDWriteTextFormat = null;
        _ = app.writeFactory.?.CreateTextFormat(
            windowName,
            null,
            win32.DWRITE_FONT_WEIGHT_REGULAR,
            win32.DWRITE_FONT_STYLE_NORMAL,
            win32.DWRITE_FONT_STRETCH_NORMAL,
            16.0,
            @as([*:0]const u16, &app.localeBuffer),
            @ptrCast(&tfOut),
        );
        app.textFormat = tfOut;

        // Now create bigTextFormat
        var tfBigOut: ?*win32.IDWriteTextFormat = null;
        _ = app.writeFactory.?.CreateTextFormat(
            windowName,
            null,
            win32.DWRITE_FONT_WEIGHT_BOLD,
            win32.DWRITE_FONT_STYLE_NORMAL,
            win32.DWRITE_FONT_STRETCH_NORMAL,
            32.0,
            @as([*:0]const u16, &app.localeBuffer),
            @ptrCast(&tfBigOut),
        );
        app.bigTextFormat = tfBigOut;

        // Choose monitor based on the cursor position
        var pt: win32.POINT = .{ .x = 0, .y = 0 };
        _ = win32.GetCursorPos(&pt);
        const hMonitor = win32.MonitorFromPoint(pt, win32.MONITOR_DEFAULTTONEAREST);

        var mi: win32.MONITORINFO = @bitCast([_]u8{0} ** (@sizeOf(win32.MONITORINFO)));
        mi.cbSize = @sizeOf(win32.MONITORINFO);
        _ = win32.GetMonitorInfoW(hMonitor, &mi);

        const activeLeft = mi.rcMonitor.left;
        const activeTop = mi.rcMonitor.top;
        const activeWidth = mi.rcMonitor.right - mi.rcMonitor.left;
        const activeHeight = mi.rcMonitor.bottom - mi.rcMonitor.top;

        // Get module handle
        const hInstance = win32.GetModuleHandleW(null);

        // Register window class
        _ = win32.RegisterClassExW(&win32.WNDCLASSEXW{
            .cbSize = @sizeOf(win32.WNDCLASSEXW),
            .style = win32.WNDCLASS_STYLES{},
            .lpfnWndProc = App.windowProcThunk,
            .hInstance = hInstance,
            .hCursor = win32.LoadCursorW(null, win32.IDC_ARROW),
            .hbrBackground = null,
            .lpszClassName = className,
            .cbClsExtra = 0,
            .cbWndExtra = 0,
            .hIcon = null,
            .lpszMenuName = null,
            .hIconSm = null,
        });

        // Create window using the same hInstance
        const hwnd = win32.CreateWindowExW(
            win32.WINDOW_EX_STYLE{},
            className,
            windowName,
            win32.WINDOW_STYLE{ .POPUP = 1 },
            activeLeft,
            activeTop,
            activeWidth,
            activeHeight,
            null,
            null,
            hInstance,
            &app,
        );
        if (hwnd == null) return error.WindowCreationFailed;
        app.hwnd = hwnd;

        // Ensure the window is actually shown and can receive keyboard focus.
        _ = win32.ShowWindow(hwnd, win32.SW_SHOW);

        // Now that we have a hwnd, ensureRenderTarget
        app.ensureRenderTarget() catch |err| {
            std.debug.print("Failed to ensure render target in init: {}\n", .{err});
        };
        try app.updateTextLayouts();

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
        if (self.redBrush != null) {
            _ = self.redBrush.?.IUnknown.Release();
            self.redBrush = null;
        }
        if (self.smallTextBitmap != null) {
            _ = self.smallTextBitmap.?.IUnknown.Release();
            self.smallTextBitmap = null;
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
                    self.textBuffer[self.textLen] = 0;
                    _ = self.updateTextLayouts() catch |err| {
                        std.debug.print("updateTextLayouts failed: {}\n", .{err});
                    };
                    _ = win32.InvalidateRect(self.hwnd, null, 1);
                }
            },
            0...7, 9, 0x0B, 0x0C, 0x0E...0x1F => {}, // Ignore other control characters
            else => { // Accept all other Unicode characters
                if (self.textLen < self.textBuffer.len - 1) { // Leave room for null terminator
                    self.textBuffer[self.textLen] = char;
                    self.textLen += 1;
                    self.textBuffer[self.textLen] = 0; // null terminate
                    _ = self.updateTextLayouts() catch |err| {
                        std.debug.print("updateTextLayouts failed: {}\n", .{err});
                    };
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

        self.ensureRenderTarget() catch |err| {
            std.debug.print("Failed to ensure render target: {}\n", .{err});
            return 0;
        };
        self.draw();

        return 0;
    }

    fn ensureRenderTarget(self: *App) !void {
        if (self.renderTarget != null) return;

        if (self.factory == null) return error.MissingFactory;

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
        const hr = self.factory.?.CreateHwndRenderTarget(
            &targetProps,
            &hwndProps,
            @ptrCast(&rtOut),
        );
        if (hr != win32.S_OK) return error.RenderTargetCreateFailed;
        if (rtOut == null) return error.RenderTargetCreateFailed;

        self.renderTarget = rtOut;

        // Create brushes
        var brushOut: ?*win32.ID2D1SolidColorBrush = null;
        _ = self.renderTarget.?.ID2D1RenderTarget.CreateSolidColorBrush(
            &.{ .r = 1, .g = 1, .b = 1, .a = 1 },
            null,
            @ptrCast(&brushOut),
        );
        if (brushOut == null) return error.BrushCreateFailed;
        self.brush = brushOut;

        var redBrushOut: ?*win32.ID2D1SolidColorBrush = null;
        _ = self.renderTarget.?.ID2D1RenderTarget.CreateSolidColorBrush(
            &.{ .r = 1, .g = 0, .b = 0, .a = 1 },
            null,
            @ptrCast(&redBrushOut),
        );
        if (redBrushOut == null) return error.BrushCreateFailed;
        self.redBrush = redBrushOut;
    }

    fn draw(self: *App) void {
        self.renderTarget.?.ID2D1RenderTarget.BeginDraw();
        defer _ = self.renderTarget.?.ID2D1RenderTarget.EndDraw(null, null);

        self.renderTarget.?.ID2D1RenderTarget.Clear(&win32.D2D_COLOR_F{ .r = 0, .g = 0, .b = 0, .a = 1 });

        var clientRect: win32.RECT = .{ .left = 0, .right = 0, .top = 0, .bottom = 0 };
        _ = win32.GetClientRect(self.hwnd, &clientRect);
        const clientWidth: f32 = @floatFromInt(clientRect.right - clientRect.left);
        const clientHeight: f32 = @floatFromInt(clientRect.bottom - clientRect.top);

        const textLen: u32 = @intCast(self.textLen);
        var bigTextHeight: f32 = 0;

        // Draw the big layout if it exists
        if (self.layoutBig != null) {
            var metrics: win32.DWRITE_TEXT_METRICS = undefined;
            _ = self.layoutBig.?.GetMetrics(&metrics);
            bigTextHeight = metrics.height;
            if (textLen > 0) {
                // Actually draw the big text
                self.renderTarget.?.ID2D1RenderTarget.DrawTextLayout(
                    .{ .x = 0, .y = 0 },
                    self.layoutBig,
                    @ptrCast(self.brush),
                    win32.D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }

            // Position the cursor at the trailing edge
            var hitTestMetrics: win32.DWRITE_HIT_TEST_METRICS = undefined;
            _ = self.layoutBig.?.HitTestTextPosition(
                textLen,
                1,
                &hitTestMetrics.left,
                &hitTestMetrics.top,
                &hitTestMetrics,
            );

            const lineHeight = hitTestMetrics.height;
            const cursorHeight: f32 = lineHeight * 0.8;
            const cursorY = hitTestMetrics.top + (lineHeight - cursorHeight) / 2;
            const cursorX = hitTestMetrics.left;

            const cursorPath = blk: {
                var geometry: ?*win32.ID2D1PathGeometry = null;
                _ = self.factory.?.CreatePathGeometry(@ptrCast(&geometry));
                var sink: ?*win32.ID2D1GeometrySink = null;
                _ = geometry.?.Open(@ptrCast(&sink));

                sink.?.ID2D1SimplifiedGeometrySink.BeginFigure(.{ .x = cursorX, .y = cursorY }, win32.D2D1_FIGURE_BEGIN_FILLED);
                sink.?.AddLine(.{ .x = cursorX + 15, .y = cursorY + cursorHeight / 2 });
                sink.?.AddLine(.{ .x = cursorX, .y = cursorY + cursorHeight });
                sink.?.ID2D1SimplifiedGeometrySink.EndFigure(win32.D2D1_FIGURE_END_CLOSED);
                _ = sink.?.ID2D1SimplifiedGeometrySink.Close();
                _ = sink.?.IUnknown.Release();
                break :blk geometry;
            };
            defer _ = cursorPath.?.IUnknown.Release();

            self.renderTarget.?.ID2D1RenderTarget.FillGeometry(
                @ptrCast(cursorPath),
                @ptrCast(self.redBrush),
                null,
            );
        }

        // Draw the small text grid if we have a smallTextBitmap
        if (self.smallTextBitmap != null and textLen > 0) {
            const cols = @as(f32, @floatFromInt(@divFloor(@as(u32, @intFromFloat(clientWidth)), @as(u32, @intFromFloat(self.smallTextWidth)))));
            const rows = @as(f32, @floatFromInt(@divFloor(@as(u32, @intFromFloat(clientHeight)), @as(u32, @intFromFloat(self.smallTextHeight)))));

            var yGrid: f32 = bigTextHeight;
            var row: f32 = 0;
            while (row < rows) : (row += 1) {
                var xGrid: f32 = (clientWidth - (cols * self.smallTextWidth)) / 2;
                var col: f32 = 0;
                while (col < cols) : (col += 1) {
                    self.renderTarget.?.ID2D1RenderTarget.DrawBitmap(
                        self.smallTextBitmap,
                        &.{
                            .left = xGrid,
                            .top = yGrid,
                            .right = xGrid + self.smallTextWidth,
                            .bottom = yGrid + self.smallTextHeight,
                        },
                        1.0,
                        win32.D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        null,
                    );
                    xGrid += self.smallTextWidth;
                }
                yGrid += self.smallTextHeight;
            }
        }
    }

    fn updateTextLayouts(self: *App) !void {
        // If we have existing layouts or the smallTextBitmap, release them so we can recreate
        if (self.layoutSmall != null) {
            _ = self.layoutSmall.?.IUnknown.Release();
            self.layoutSmall = null;
        }
        if (self.smallTextBitmap != null) {
            _ = self.smallTextBitmap.?.IUnknown.Release();
            self.smallTextBitmap = null;
        }

        // Build the big layout (for the main text)
        const textPtr = @as([*:0]const u16, &self.textBuffer);
        const textLen: u32 = @intCast(self.textLen);

        var layoutBigOut: ?*win32.IDWriteTextLayout = null;
        // Safety check: self.bigTextFormat might be null if creation failed during init
        if (self.bigTextFormat != null) {
            _ = self.writeFactory.?.CreateTextLayout(
                textPtr,
                textLen,
                self.bigTextFormat,
                1920.0, // some large width, or pass actual client width
                1080.0, // some large height, or pass actual client height
                @ptrCast(&layoutBigOut),
            );
        }
        // store the layout in a new field layoutBig
        if (self.layoutBig != null) {
            _ = self.layoutBig.?.IUnknown.Release();
        }
        self.layoutBig = layoutBigOut;

        // Build the small text layout
        if (self.textFormat != null and textLen > 0) {
            var layoutSmallOut: ?*win32.IDWriteTextLayout = null;
            _ = self.writeFactory.?.CreateTextLayout(
                textPtr,
                textLen,
                self.textFormat,
                1920.0,
                1080.0,
                @ptrCast(&layoutSmallOut),
            );
            self.layoutSmall = layoutSmallOut;

            if (self.layoutSmall != null) {
                var smallMetrics: win32.DWRITE_TEXT_METRICS = undefined;
                _ = self.layoutSmall.?.GetMetrics(&smallMetrics);
                self.smallTextWidth = smallMetrics.widthIncludingTrailingWhitespace;
                self.smallTextHeight = smallMetrics.height;

                // If we have a valid renderTarget, create the smallTextBitmap once
                if (self.renderTarget != null and self.smallTextWidth > 0 and self.smallTextHeight > 0) {
                    var bitmapRenderTarget: ?*win32.ID2D1BitmapRenderTarget = null;
                    const hr1 = self.renderTarget.?.ID2D1RenderTarget.CreateCompatibleRenderTarget(
                        &win32.D2D_SIZE_F{ .width = self.smallTextWidth, .height = self.smallTextHeight },
                        null,
                        &win32.D2D1_PIXEL_FORMAT{
                            .format = win32.DXGI_FORMAT_B8G8R8A8_UNORM,
                            .alphaMode = win32.D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        win32.D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
                        @ptrCast(&bitmapRenderTarget),
                    );
                    if (hr1 == win32.S_OK and bitmapRenderTarget != null) {
                        // Create brush for bitmap target
                        var bitmapBrush: ?*win32.ID2D1SolidColorBrush = null;
                        const hr2 = bitmapRenderTarget.?.ID2D1RenderTarget.CreateSolidColorBrush(
                            &.{ .r = 1, .g = 1, .b = 1, .a = 1 },
                            null,
                            @ptrCast(&bitmapBrush),
                        );
                        if (hr2 == win32.S_OK and bitmapBrush != null) {
                            defer _ = bitmapBrush.?.IUnknown.Release();
                            // Draw text into bitmap render target
                            bitmapRenderTarget.?.ID2D1RenderTarget.BeginDraw();
                            bitmapRenderTarget.?.ID2D1RenderTarget.Clear(&.{ .r = 0, .g = 0, .b = 0, .a = 0 });
                            bitmapRenderTarget.?.ID2D1RenderTarget.DrawTextLayout(
                                .{ .x = 0, .y = 0 },
                                self.layoutSmall,
                                @ptrCast(bitmapBrush),
                                win32.D2D1_DRAW_TEXT_OPTIONS_NONE,
                            );
                            _ = bitmapRenderTarget.?.ID2D1RenderTarget.EndDraw(null, null);
                            // Get bitmap from render target
                            var bitmap: ?*win32.ID2D1Bitmap = null;
                            const hr4 = bitmapRenderTarget.?.GetBitmap(@ptrCast(&bitmap));
                            if (hr4 == win32.S_OK and bitmap != null) {
                                self.smallTextBitmap = bitmap;
                            }
                        }
                        _ = bitmapRenderTarget.?.IUnknown.Release();
                    }
                }
            }
        }
    }
};

pub fn main() !void {
    var app = try App.init();
    defer app.deinit();
    var msg: win32.MSG = undefined;
    while (win32.GetMessageW(&msg, null, 0, 0) != 0) {
        _ = win32.TranslateMessage(&msg);
        _ = win32.DispatchMessageW(&msg);
    }
}
