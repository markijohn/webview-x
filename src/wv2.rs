use webview2;
use once_cell::unsync::OnceCell;
use std::mem;
use std::ptr;
use std::rc::Rc;
use webview2::Controller;
use winapi::{
    shared::minwindef::*, shared::windef::*, um::libloaderapi::*, um::winbase::MulDiv,
    um::wingdi::*, um::winuser::*,
};

fn utf_16_null_terminiated(x: &str) -> Vec<u16> {
    x.encode_utf16().chain(std::iter::once(0)).collect()
}
fn message_box(hwnd: HWND, text: &str, caption: &str, _type: u32) -> i32 {
    let text = utf_16_null_terminiated(text);
    let caption = utf_16_null_terminiated(caption);

    unsafe { MessageBoxW(hwnd, text.as_ptr(), caption.as_ptr(), _type) }
}

pub struct WebView2 {

}

impl WebView2 {
    pub fn new() {
        //set dpi aware
        unsafe {
            // Windows 10.
            let user32 = LoadLibraryA(b"user32.dll\0".as_ptr() as *const i8);
            let set_thread_dpi_awareness_context = GetProcAddress(
                user32,
                b"SetThreadDpiAwarenessContext\0".as_ptr() as *const i8,
            );
            if !set_thread_dpi_awareness_context.is_null() {
                let set_thread_dpi_awareness_context: extern "system" fn(
                    DPI_AWARENESS_CONTEXT,
                )
                    -> DPI_AWARENESS_CONTEXT = mem::transmute(set_thread_dpi_awareness_context);
                set_thread_dpi_awareness_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            } else {
                // Windows 7.
                SetProcessDPIAware();
            }
        }

        let controller = Rc::new(OnceCell::<Controller>::new());
        let controller_clone = controller.clone();

        // Window procedure.
        let wnd_proc = move |hwnd, msg, w_param, l_param| match msg {
            WM_SIZE => {
                if let Some(c) = controller.get() {
                    let mut r = unsafe { mem::zeroed() };
                    unsafe {
                        GetClientRect(hwnd, &mut r);
                    }
                    c.put_bounds(r).unwrap();
                }
                0
            }
            WM_MOVE => {
                if let Some(c) = controller.get() {
                    let _ = c.notify_parent_window_position_changed();
                }
                0
            }
            // Optimization: don't render the webview when the window is minimized.
            WM_SYSCOMMAND if w_param == SC_MINIMIZE => {
                if let Some(c) = controller.get() {
                    c.put_is_visible(false).unwrap();
                }
                unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) }
            }
            WM_SYSCOMMAND if w_param == SC_RESTORE => {
                if let Some(c) = controller.get() {
                    c.put_is_visible(true).unwrap();
                }
                unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) }
            }
            // High DPI support.
            WM_DPICHANGED => unsafe {
                let rect = *(l_param as *const RECT);
                SetWindowPos(
                    hwnd,
                    ptr::null_mut(),
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                0
            },
            _ => unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) },
        };

        // Register window class. (Standard windows GUI boilerplate).
        let class_name = utf_16_null_terminiated("WebView2 Win32 Class");
        let h_instance = unsafe { GetModuleHandleW(ptr::null()) };
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            lpfnWndProc: Some(unsafe { wnd_proc_helper::as_global_wnd_proc(wnd_proc) }),
            lpszClassName: class_name.as_ptr(),
            hInstance: h_instance,
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            ..unsafe { mem::zeroed() }
        };
        unsafe {
            if RegisterClassW(&class) == 0 {
                message_box(
                    ptr::null_mut(),
                    &format!("RegisterClassW failed: {}", std::io::Error::last_os_error()),
                    "Error",
                    MB_ICONERROR | MB_OK,
                );
                return Err(WVError::Cause("RegisterClassW failed"))
            }
        }

        // Create window. (Standard windows GUI boilerplate).
        let window_title = utf_16_null_terminiated("WebView2 - Win 32");
        let hdc = unsafe { GetDC(ptr::null_mut()) };
        let dpi = unsafe { GetDeviceCaps(hdc, LOGPIXELSX) };
        unsafe { ReleaseDC(ptr::null_mut(), hdc) };
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                window_title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                MulDiv(width, dpi, USER_DEFAULT_SCREEN_DPI),
                MulDiv(height, dpi, USER_DEFAULT_SCREEN_DPI),
                ptr::null_mut(),
                ptr::null_mut(),
                h_instance,
                ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            message_box(
                ptr::null_mut(),
                &format!(
                    "CreateWindowExW failed: {}",
                    std::io::Error::last_os_error()
                ),
                "Error",
                MB_ICONERROR | MB_OK,
            );
            return Err(WVError::Cause("CreateWindowExW failed"))
        }
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
        }

        // Create the webview.
        let r = webview2::Environment::builder().build(move |env| {
            env.unwrap().create_controller(hwnd, move |c| {
                let c = c.unwrap();

                let mut r = unsafe { mem::zeroed() };
                unsafe {
                    GetClientRect(hwnd, &mut r);
                }
                c.put_bounds(r).unwrap();

                let w = c.get_webview().unwrap();
                // Communication.
                w.navigate_to_string(r##"
<!doctype html>
<title>Demo</title>
<form action="javascript:void(0);">
    <label for="message-input">Message: </label
    ><input id="message-input" type="text"
    ><button type="submit">Send</button>
</form>
<script>
const inputElement = document.getElementById('message-input');
document.getElementsByTagName('form')[0].addEventListener('submit', e => {
    // Send message to host.
    window.chrome.webview.postMessage(inputElement.value);
});
// Receive from host.
window.chrome.webview.addEventListener('message', event => alert('Received message: ' + event.data));
</script>
"##).unwrap();
                // Receive message from webpage.
                w.add_web_message_received(|w, msg| {
                    let msg = msg.try_get_web_message_as_string()?;
                    // Send it back.
                    w.post_web_message_as_string(&msg)
                }).unwrap();
                controller_clone.set(c).unwrap();
                Ok(())
            })
        });
        if let Err(e) = r {
            message_box(
                ptr::null_mut(),
                &format!("Creating WebView2 Environment failed: {}\n", e),
                "Error",
                MB_ICONERROR | MB_OK,
            );
            return Err(WVError::Cause("Creating WebView2 Environment failed"));
        }
    }
}