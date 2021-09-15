use std::error::Error;
use winapi::shared::windef::HWND;
use std::ffi::{CStr, CString};
use std::path::Path;
use std::fmt::{Debug, Formatter, Display};
mod wv2;

#[derive(Debug,Di)]
pub enum WVError {
    Cause(&'static str)
}

impl Display for WVError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}",self)
    }
}

impl std::error::Error for WVError { }

pub fn install_webview2(confirm:Option<&str>, wv2_folder:Option<&Path>) -> bool {
    if webview2::get_available_browser_version_string(wv2_folder).is_err() {
        use std::io::Write;
        use std::os::windows::process::CommandExt;

        if let Some(m) = confirm {
            use tinyfiledialogs::*;
            if let OkCancel::Cancel = message_box_ok_cancel("", m, MessageBoxIcon::Question, OkCancel::Ok) {
                return false
            }
        }

        // Run a powershell script to install the WebView2 runtime.
        //
        // Use powershell instead of a rust http library like ureq because using
        // the latter makes the executable file a lot bigger (~500KiB).
        let mut p = std::process::Command::new("powershell.exe")
            .arg("-Command")
            .arg("-")
            // Let powershell open its own console window.
            .creation_flags(/*CREATE_NEW_CONSOLE*/ 0x00000010)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = p.stdin.take().unwrap();
        stdin
            .write_all(include_bytes!("download-and-run-bootstrapper.ps1"))
            .unwrap();
        drop(stdin);
        let r = p.wait().unwrap();
        r.success()
    } else {
        true
    }
}

pub type WVResult<T=()> = Result<T,WVError>;

#[derive(Copy,Clone)]
pub enum WebViewEngine {
    ///Suggestion webview2
    /// ex:)
    /// Auto(Some("WebView2 is not installed. WebView2 will provide a better experience. WebView2 will provide a better experience.")) => Suggestion install webview2. if installation failed then fallback to legaycy mode
    /// Auto(None) => Not suggestion installing webview2 but try install. if installation failed then fallback to legaycy mode
    Auto(Option<&'static str>),

    ///if webview2 not available then we use legacy MSHTML
    Fallback,

    ///Force legacy MSHTML
    Legacy,

    ///Force webview2
    WebView2(Option<&'static str>)
}

pub struct WebViewLegacyHolder<'a> {
    title : &'a str
}

pub struct WebView2Holder<'a> {
    title : &'a str,
    hwnd : HWND
}

pub enum WebViewInfoHolder<'a> {
    Legacy(WebViewLegacyHolder<'a>),
    WebView2(WebView2Holder<'a>)
}

impl <'a> WebViewInfoHolder<'_> {
    pub fn set_title(&mut self, t:&'a str) {
        match self {
            WebViewInfoHolder::Legacy(v) => { v.title = t; }
            WebViewInfoHolder::WebView2(v) => { v.title = t; }
        }
    }
}

impl Default for WebViewInfoHolder {
    fn default() -> Self {
        Self { title : "No title" }
    }
}

pub struct WebViewBuilder<'a> {
    pub engine : WebViewEngine,
    pub holder: WebViewInfoHolder<'a>,
    pub url : &'a str,
    pub debug : bool,
    pub width: i32,
    pub height: i32,
    pub resizable: bool,
    pub invoke_handler: Option<fn (&mut WebView, data:&str)>,
    pub frameless: bool,
}

impl Default for WebViewBuilder {
    fn default() -> Self {
        WebViewBuilder {
            engine : WebViewEngine::Auto(Some("")),
            holder : WebViewInfoHolder::default(),
            url : "about:blank",
            debug : false,
            width: 800,
            height: 600,
            resizable: true,
            invoke_handler: None,
            frameless: false,
        }
    }
}

impl WebViewBuilder {
    /// Alias for [`WebViewBuilder::default()`].
    ///
    /// [`WebViewBuilder::default()`]: struct.WebviewBuilder.html#impl-Default
    pub fn new() -> Self {
        WebViewBuilder::default()
    }

    /// Sets the title of the WebView window.
    ///
    /// Defaults to `"Application"`.
    pub fn title(mut self, title: & str) -> Self {
        self.holder.set_title( title );
        self
    }

    /// Sets the size of the WebView window.
    ///
    /// Defaults to 800 x 600.
    pub fn size(mut self, width: i32, height: i32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Sets the resizability of the WebView window. If set to false, the window cannot be resized.
    ///
    /// Defaults to `true`.
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// The window crated will be frameless
    ///
    /// defaults to `false`
    pub fn frameless(mut self, frameless: bool) -> Self {
        self.frameless = frameless;
        self
    }

    /// Sets the invoke handler callback. This will be called when a message is received from
    /// JavaScript.
    ///
    /// # Errors
    ///
    /// If the closure returns an `Err`, it will be returned on the next call to [`step()`].
    ///
    /// [`step()`]: struct.WebView.html#method.step
    pub fn invoke_handler(mut self, invoke_handler: I) -> Self {
        self.invoke_handler = Some(invoke_handler);
        self
    }

    /// Sets the initial state of the user data. This is an arbitrary value stored on the WebView
    /// thread, accessible from dispatched closures without synchronization overhead.
    pub fn user_data(mut self, user_data: T) -> Self {
        self.user_data = Some(user_data);
        self
    }

    /// Validates provided arguments and returns a new WebView if successful.
    pub fn build(self) -> WVResult<WebView> {
        let wv2_installed = match self.engine {
            WebViewEngine::WebView2(msg) => {
                if !install_webview2(msg, None) {
                    return Err(WVRe)
                }
                true
            }
            WebViewEngine::Auto(msg) => {
                install_webview2(msg, None)
            }
            WebViewEngine::Fallback => {
                webview2::get_available_browser_version_string(wv2_folder).is_ok()
            }
            _ => None
        };

        if wv2_installed {
            //we can use webview2
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


        }

        if let Some(wv2) = wv2 {
            Err( std::io::Error )
        } else if wv2.is_none() &&
            (self.engine == WebViewEngine::Auto || self.engine == WebViewEngine::Legacy) {
            let url = if &self.url[ .. 10.min(self.url.len()-1)].find("://").is_none() {
                web_view::Content::Html( self.url )
            } else {
                web_view::Content::Url( self.url )
            };
            let wv_legacy = web_view::WebViewBuilder::new()
                .title( self.title )
                .content( url )
                .size( self.width, self.height )
                .resizable( self.resizable )
                .debug( self.debug )
                .frameless( self.frameless )
                .user_data( () )
                .invoke_handler( |_,_| { Ok(())} )
                .build()?;
            Ok( WebView::WV1( (self.holder.clone(),wv_legacy) ) )
        } else {
            Ok( wv2? )
        }

    }

    /// Validates provided arguments and runs a new WebView to completion, returning the user data.
    ///
    /// Equivalent to `build()?.run()`.
    pub fn run(self) {
        self.build()?.run()
    }
}


enum WebView<'a> {
    WV1( (WebViewLegacyHolder<'a>, web_view::WebView<'a, ()>) ),
    WV2( (WebView2Holder<'a>, webview2::WebView) )
}

impl <'a> WebView<'a> {
    pub fn step(&mut self) {
        match self {
            WebView::WV1( (_,wv)) => {
                wv.step();
            }
            WebView::WV2( (h,wv) ) => {

            }
        }
    }
}