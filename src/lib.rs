use std::error::Error;
use winapi::shared::windef::HWND;
use std::ffi::{CStr, CString};
use std::path::Path;
use std::fmt::{Debug, Formatter, Display};
mod wv2;

#[derive(Debug)]
pub enum WVError {
    Cause(&'static str)
}

impl Display for WVError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}",self)
    }
}

impl std::error::Error for WVError { }

impl From<web_view::Error> for WVError {
    fn from(_: web_view::Error) -> Self {
        Self::Cause("wv1 error")
    }
}

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
            .write_all(include_bytes!("../download-and-run-bootstrapper.ps1"))
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
pub enum WebViewMode {
    ///Suggestion webview2
    /// ex:)
    /// Auto(Some("WebView2 is not installed. WebView2 will provide a better experience. Do you want install?")) => Suggestion install webview2. if installation failed then fallback to legaycy mode
    /// Auto(None) => Not suggestion installing webview2 but try install. if installation failed then fallback to legaycy mode
    Auto(Option<&'static str>),

    ///if webview2 not available then we use legacy MSHTML
    Fallback,

    ///Force legacy MSHTML
    MSHTML,

    ///Force webview2
    WebView2(Option<&'static str>)
}

pub struct WebViewBuilder<'a> {
    pub engine : WebViewMode,
    pub title : &'a str,
    pub url : &'a str,
    pub debug : bool,
    pub width: i32,
    pub height: i32,
    pub resizable: bool,
    pub invoke_handler: Option<fn (&mut WebView, data:&str)>,
    pub frameless: bool,
}

impl <'a> Default for WebViewBuilder<'_> {
    fn default() -> Self {
        WebViewBuilder {
            engine : WebViewMode::Auto(Some("")),
            title : "No title",
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

impl <'a> WebViewBuilder<'a> {
    /// Alias for [`WebViewBuilder::default()`].
    ///
    /// [`WebViewBuilder::default()`]: struct.WebviewBuilder.html#impl-Default
    pub fn new() -> Self {
        WebViewBuilder::default()
    }

    pub fn mode(mut self, mode:WebViewMode) -> Self {
        self.engine = mode;
        self
    }

    /// Sets the title of the WebView window.
    ///
    /// Defaults to `"Application"`.
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
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

    pub fn url(mut self, url:&'a str) -> Self {
        self.url = url;
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
    pub fn invoke_handler(mut self, invoke_handler: fn(&mut WebView, data:&str)) -> Self {
        self.invoke_handler = Some(invoke_handler);
        self
    }

    /// Validates provided arguments and returns a new WebView if successful.
    pub fn build(self) -> WVResult<WebView<'a>> {
        let wv2_installed = match self.engine {
            WebViewMode::WebView2(msg) => {
                if !install_webview2(msg, None) {
                    return Err(WVError::Cause("webview2 install failed"))
                }
                true
            }
            WebViewMode::Auto(msg) => {
                install_webview2(msg, None)
            }
            WebViewMode::Fallback => {
                webview2::get_available_browser_version_string(None).is_ok()
            }
            _ => false
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

            let wv2 = wv2::WebView2Builder::new()
                .title( self.title )
                .url( self.url )
                .size( self.width, self.height )
                .resizable( self.resizable )
                .build()?;

            return Ok(
                WebView::WV2(wv2)
            )
        }

        let url = if self.url[ .. 10.min(self.url.len()-1)].find("://").is_none() {
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
        Ok( WebView::WV1( wv_legacy ) )

    }
}


pub enum WebView<'a> {
    WV1( web_view::WebView<'a, ()> ),
    WV2( wv2::WebView2 )
}

impl <'a> WebView<'a> {
    pub fn step(&mut self) {
        match self {
            WebView::WV1( wv) => {
                wv.step();
            }
            WebView::WV2( wv) => {
                wv.step();
            }
        }
    }

    pub fn exit(&mut self) {
        match self {
            WebView::WV1( wv) => {
                wv.exit();
            }
            WebView::WV2( wv) => {
                wv.exit();
            }
        }
    }
}