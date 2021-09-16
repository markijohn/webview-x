use once_cell::unsync::OnceCell;
use std::mem;
use std::ptr;
use std::rc::Rc;
use webview2::Controller;
use winapi::{
    shared::minwindef::*, shared::windef::*, um::libloaderapi::GetModuleHandleW, um::winuser::*,
};

fn main() {
    println!("{:?}", webview2::get_available_browser_version_string(None));

    let wv = webview_x::WebViewBuilder::new()
        .build();

    // Message loop. (Standard windows GUI boilerplate).
    let mut msg: MSG = unsafe { mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}