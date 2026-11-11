//! Talks to the caption service (captionistd) over D-Bus.

use adw::prelude::*;
use gtk::{gio, glib};

pub const NAME: &str = "io.github.m_umair_khan_dev.Captionist.Daemon";
pub const PATH: &str = "/io/github/m_umair_khan_dev/Captionist/Daemon";
pub const IFACE: &str = "io.github.m_umair_khan_dev.Captionist.Daemon1";

#[derive(Clone)]
pub struct Client {
    proxy: gio::DBusProxy,
}

impl Client {
    /// Connects (starting the service if needed).
    pub fn connect() -> Result<Self, glib::Error> {
        let proxy = gio::DBusProxy::for_bus_sync(
            gio::BusType::Session,
            gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
            None,
            NAME,
            PATH,
            IFACE,
            gio::Cancellable::NONE,
        )?;
        let c = Client { proxy };
        // Make sure the service is running so its signals reach us.
        let _ = c.proxy.call_sync(
            "Ping",
            None,
            gio::DBusCallFlags::NONE,
            10_000,
            gio::Cancellable::NONE,
        );
        Ok(c)
    }

    pub async fn call(
        &self,
        method: &str,
        args: Option<glib::Variant>,
    ) -> Result<glib::Variant, glib::Error> {
        self.proxy
            .call_future(method, args.as_ref(), gio::DBusCallFlags::NONE, 120_000)
            .await
    }

    pub async fn call_string(
        &self,
        method: &str,
        args: Option<glib::Variant>,
    ) -> Result<String, glib::Error> {
        let v = self.call(method, args).await?;
        Ok(v.get::<(String,)>().map(|t| t.0).unwrap_or_default())
    }

    /// Fire-and-forget call; errors go to `on_error`.
    pub fn spawn_call(
        &self,
        method: &'static str,
        args: Option<glib::Variant>,
        on_error: impl Fn(String) + 'static,
    ) {
        let c = self.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = c.call(method, args).await {
                on_error(e.message().to_string());
            }
        });
    }

    /// Calls `f(signal_name, parameters)` for every signal from the service.
    pub fn on_signal(&self, f: impl Fn(&str, &glib::Variant) + 'static) {
        self.proxy.connect_local("g-signal", false, move |vals| {
            let name: String = vals.get(2).and_then(|v| v.get().ok()).unwrap_or_default();
            if let Some(params) = vals.get(3).and_then(|v| v.get::<glib::Variant>().ok()) {
                f(&name, &params);
            }
            None
        });
    }
}
