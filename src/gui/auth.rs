use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{CStr, c_char},
    rc::Rc,
    sync::OnceLock,
};

use gtk4::{
    gio, glib,
    glib::{translate::*, variant::ToVariant},
    prelude::*,
};
use libloading::Library;

use crate::core::i18n::{tr, trf};

const SERVICE: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const AUTHORITY: &str = "org.freedesktop.PolicyKit1.Authority";
const AGENT_PATH: &str = "/io/github/eulucastiagolt/lenvix/AuthenticationAgent";
const AGENT: &str = "org.freedesktop.PolicyKit1.AuthenticationAgent";
const CANCELLED: &str = "org.freedesktop.PolicyKit1.Error.Cancelled";
const FAILED: &str = "org.freedesktop.PolicyKit1.Error.Failed";
const INTERFACE: &str = r#"
<node>
  <interface name="org.freedesktop.PolicyKit1.AuthenticationAgent">
    <method name="BeginAuthentication">
      <arg type="s" direction="in"/>
      <arg type="s" direction="in"/>
      <arg type="s" direction="in"/>
      <arg type="a{ss}" direction="in"/>
      <arg type="s" direction="in"/>
      <arg type="a(sa{sv})" direction="in"/>
    </method>
    <method name="CancelAuthentication">
      <arg type="s" direction="in"/>
    </method>
  </interface>
</node>
"#;

type ObjectPtr = *mut glib::gobject_ffi::GObject;
type SessionAction = unsafe extern "C" fn(ObjectPtr);
type Details = HashMap<String, glib::Variant>;
type Challenge = (
    String,
    String,
    String,
    HashMap<String, String>,
    String,
    Vec<(String, Details)>,
);

struct Polkit {
    _agent: Library,
    _identity: Library,
    new_user: unsafe extern "C" fn(i32) -> ObjectPtr,
    user_name: unsafe extern "C" fn(ObjectPtr) -> *const c_char,
    new_session: unsafe extern "C" fn(ObjectPtr, *const c_char) -> ObjectPtr,
    initiate: SessionAction,
    cancel: SessionAction,
    respond: unsafe extern "C" fn(ObjectPtr, *const c_char),
}

impl Polkit {
    fn load() -> Result<Self, libloading::Error> {
        unsafe {
            let agent = Library::new("libpolkit-agent-1.so.0")?;
            let identity = Library::new("libpolkit-gobject-1.so.0")?;
            Ok(Self {
                new_user: *identity.get(b"polkit_unix_user_new")?,
                user_name: *identity.get(b"polkit_unix_user_get_name")?,
                new_session: *agent.get(b"polkit_agent_session_new")?,
                initiate: *agent.get(b"polkit_agent_session_initiate")?,
                cancel: *agent.get(b"polkit_agent_session_cancel")?,
                respond: *agent.get(b"polkit_agent_session_response")?,
                _agent: agent,
                _identity: identity,
            })
        }
    }
}

static POLKIT: OnceLock<Result<Polkit, libloading::Error>> = OnceLock::new();

#[derive(Default)]
struct Authentication {
    cookie: String,
    invocation: Option<gio::DBusMethodInvocation>,
    session: Option<glib::Object>,
    dialog: Option<gtk4::Window>,
    status: Option<Rc<dyn Fn(bool)>>,
}

impl Authentication {
    fn finish(&mut self, error: Option<(&str, &str)>) {
        if let Some(dialog) = self.dialog.take() {
            dialog.destroy();
        }
        if let Some(invocation) = self.invocation.take() {
            if let Some((name, message)) = error {
                invocation.return_dbus_error(name, message);
            } else {
                invocation.return_value(None);
            }
        }
        self.session.take();
        self.cookie.clear();
        if let Some(status) = self.status.take() {
            status(false);
        }
    }
}

pub(super) struct Agent {
    bus: gio::DBusConnection,
    registration: Option<gio::RegistrationId>,
    authority: String,
    subject: glib::Variant,
    registered: bool,
    active: Rc<RefCell<Authentication>>,
    polkit: &'static Polkit,
}

impl Agent {
    pub(super) fn register(
        window: &gtk4::ApplicationWindow,
        settings: &gtk4::Window,
        status: impl Fn(bool) + 'static,
    ) -> Result<Self, String> {
        let polkit = POLKIT
            .get_or_init(Polkit::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        let bus = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE)
            .map_err(|err| err.to_string())?;
        let authority = bus
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "GetNameOwner",
                Some(&(SERVICE,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                3000,
                gio::Cancellable::NONE,
            )
            .map_err(|err| err.to_string())?
            .get::<(String,)>()
            .ok_or("Invalid Polkit bus owner")?
            .0;
        let stat = std::fs::read_to_string("/proc/self/stat").map_err(|err| err.to_string())?;
        let start_time = stat
            .rsplit_once(')')
            .and_then(|(_, fields)| fields.split_whitespace().nth(19))
            .and_then(|field| field.parse::<u64>().ok())
            .ok_or("Could not read process start time")?;
        let program = std::env::current_exe().map_err(|err| err.to_string())?;
        let info = gio::DBusNodeInfo::for_xml(INTERFACE).map_err(|err| err.to_string())?;
        let interface = info
            .lookup_interface(AGENT)
            .ok_or("Missing Polkit interface")?;
        let active = Rc::new(RefCell::new(Authentication::default()));
        let pending = Rc::clone(&active);
        let status: Rc<dyn Fn(bool)> = Rc::new(status);
        let owner = authority.clone();
        let window = window.downgrade();
        let settings = settings.downgrade();
        let registration = bus
            .register_object(AGENT_PATH, &interface)
            .method_call(move |_, sender, _, _, method, params, invocation| {
                if sender != Some(owner.as_str()) {
                    invocation.return_dbus_error(FAILED, "Unexpected sender");
                    return;
                }
                if method == "CancelAuthentication" {
                    if let Some((cookie,)) = params.get::<(String,)>()
                        && pending.borrow().cookie == cookie
                    {
                        cancel(polkit, &pending);
                    }
                    invocation.return_value(None);
                    return;
                }
                let Some((action, _, _, details, cookie, users)) = params.get::<Challenge>() else {
                    invocation.return_dbus_error(FAILED, "Invalid authentication request");
                    return;
                };
                let matches_process = match details.get("polkit.subject-pid") {
                    Some(pid) => pid.parse::<u32>() == Ok(std::process::id()),
                    None => {
                        details.get("program").map(std::path::Path::new) == Some(program.as_path())
                    }
                };
                if action != "org.freedesktop.policykit.exec"
                    || pending.borrow().invocation.is_some()
                    || !matches_process
                {
                    crate::core::log::log(
                        "WARN",
                        "Polkit: authorization challenge did not match the webcam request",
                    );
                    invocation.return_dbus_error(FAILED, "Unexpected authorization request");
                    return;
                }
                let Some(window) = window.upgrade() else {
                    invocation.return_dbus_error(CANCELLED, "Application is closing");
                    return;
                };
                let parent = settings
                    .upgrade()
                    .filter(|w| w.is_visible())
                    .unwrap_or_else(|| window.upcast());
                let identities: Vec<_> = users
                    .into_iter()
                    .filter_map(|(kind, details)| {
                        let uid = details.get("uid")?.get::<u32>()?;
                        if kind != "unix-user" || uid > i32::MAX as u32 {
                            return None;
                        }
                        unsafe {
                            let identity: glib::Object =
                                from_glib_full((polkit.new_user)(uid as i32));
                            let name = (polkit.user_name)(identity.to_glib_none().0);
                            if name.is_null() {
                                return None;
                            }
                            Some((
                                CStr::from_ptr(name).to_string_lossy().into_owned(),
                                identity,
                            ))
                        }
                    })
                    .collect();
                if identities.is_empty() {
                    invocation.return_dbus_error(FAILED, "No administrator account available");
                    return;
                }
                *pending.borrow_mut() = Authentication {
                    cookie,
                    invocation: Some(invocation),
                    status: Some(Rc::clone(&status)),
                    ..Default::default()
                };
                status(true);
                show_dialog(polkit, &pending, &parent, identities);
            })
            .build()
            .map_err(|err| err.to_string())?;

        let subject = (
            "unix-process",
            HashMap::from([
                ("pid", std::process::id().to_variant()),
                ("start-time", start_time.to_variant()),
                ("uid", (unsafe { libc::getuid() } as i32).to_variant()),
            ]),
        )
            .to_variant();
        let mut agent = Self {
            bus,
            registration: Some(registration),
            authority,
            subject,
            registered: false,
            active,
            polkit,
        };
        agent.call(
            "RegisterAuthenticationAgent",
            glib::Variant::tuple_from_iter([
                agent.subject.clone(),
                crate::core::i18n::lang().to_variant(),
                AGENT_PATH.to_variant(),
            ]),
        )?;
        agent.registered = true;
        Ok(agent)
    }

    fn call(&self, method: &str, params: glib::Variant) -> Result<(), String> {
        self.bus
            .call_sync(
                Some(&self.authority),
                AUTHORITY_PATH,
                AUTHORITY,
                method,
                Some(&params),
                None,
                gio::DBusCallFlags::NONE,
                3000,
                gio::Cancellable::NONE,
            )
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        cancel(self.polkit, &self.active);
        if let Some(registration) = self.registration.take() {
            let _ = self.bus.unregister_object(registration);
        }
        if self.registered {
            let _ = self.call(
                "UnregisterAuthenticationAgent",
                glib::Variant::tuple_from_iter([self.subject.clone(), AGENT_PATH.to_variant()]),
            );
        }
    }
}

fn cancel(polkit: &Polkit, active: &RefCell<Authentication>) {
    let session = active.borrow().session.clone();
    active
        .borrow_mut()
        .finish(Some((CANCELLED, "Authentication cancelled")));
    if let Some(session) = session {
        unsafe { (polkit.cancel)(session.to_glib_none().0) };
    }
}

fn show_dialog(
    polkit: &'static Polkit,
    active: &Rc<RefCell<Authentication>>,
    parent: &gtk4::Window,
    identities: Vec<(String, glib::Object)>,
) {
    let title = tr("Authorize the virtual webcam");
    let dialog = gtk4::Window::builder()
        .title(&title)
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .default_width(400)
        .build();
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);
    let description = gtk4::Label::new(Some(&tr(
        "Lenvix needs administrator permission to set up the virtual webcam.",
    )));
    description.set_wrap(true);
    description.set_xalign(0.0);
    content.append(&description);

    let names: Vec<_> = identities.iter().map(|(name, _)| name.as_str()).collect();
    let accounts = gtk4::DropDown::from_strings(&names);
    let current_user = glib::user_name();
    if let Some(index) = names.iter().position(|name| current_user == *name) {
        accounts.set_selected(index as u32);
    }
    let account_label = gtk4::Label::new(Some(&tr("Administrator account")));
    account_label.set_xalign(0.0);
    accounts.update_relation(&[gtk4::accessible::Relation::LabelledBy(&[
        account_label.upcast_ref()
    ])]);
    content.append(&account_label);
    content.append(&accounts);

    let prompt = gtk4::Label::new(None);
    prompt.set_xalign(0.0);
    prompt.set_wrap(true);
    content.append(&prompt);
    let entry = gtk4::Entry::builder()
        .visibility(false)
        .activates_default(true)
        .sensitive(false)
        .build();
    entry.update_relation(&[gtk4::accessible::Relation::LabelledBy(&[
        prompt.upcast_ref()
    ])]);
    content.append(&entry);
    let message = gtk4::Label::new(None);
    message.set_wrap(true);
    message.set_xalign(0.0);
    message.set_visible(false);
    content.append(&message);

    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    buttons.set_halign(gtk4::Align::End);
    let dismiss = gtk4::Button::with_label(&tr("Cancel"));
    let authorize = gtk4::Button::with_label(&tr("Authenticate"));
    authorize.add_css_class("suggested-action");
    authorize.set_sensitive(false);
    buttons.append(&dismiss);
    buttons.append(&authorize);
    content.append(&buttons);
    dialog.set_child(Some(&content));
    dialog.set_default_widget(Some(&authorize));

    let pending = Rc::downgrade(active);
    dialog.connect_close_request(move |_| {
        if let Some(active) = pending.upgrade() {
            cancel(polkit, &active);
        }
        glib::Propagation::Stop
    });
    let pending = Rc::downgrade(active);
    dismiss.connect_clicked(move |_| {
        if let Some(active) = pending.upgrade() {
            cancel(polkit, &active);
        }
    });
    let keys = gtk4::EventControllerKey::new();
    let pending = Rc::downgrade(active);
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            if let Some(active) = pending.upgrade() {
                cancel(polkit, &active);
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    dialog.add_controller(keys);

    let pending = Rc::downgrade(active);
    let field = entry.clone();
    authorize.connect_clicked(move |button| {
        let Some(session) = pending
            .upgrade()
            .and_then(|state| state.borrow().session.clone())
        else {
            return;
        };
        button.set_sensitive(false);
        field.set_sensitive(false);
        let response = field.text();
        field.set_text("");
        unsafe { (polkit.respond)(session.to_glib_none().0, response.as_ptr()) };
    });

    let pending = Rc::downgrade(active);
    let start = move |accounts: &gtk4::DropDown| {
        let Some(active) = pending.upgrade() else {
            return;
        };
        let Some((_, identity)) = identities.get(accounts.selected() as usize) else {
            return;
        };
        let old_session = active.borrow_mut().session.take();
        if let Some(old_session) = old_session {
            unsafe { (polkit.cancel)(old_session.to_glib_none().0) };
        }
        entry.set_text("");
        entry.set_sensitive(false);
        authorize.set_sensitive(false);
        message.set_visible(false);
        prompt.set_text(&tr("Waiting for the system…"));

        let cookie = glib::GString::from(active.borrow().cookie.as_str());
        let session: glib::Object = unsafe {
            from_glib_full((polkit.new_session)(
                identity.to_glib_none().0,
                cookie.as_ptr(),
            ))
        };
        let field = entry.clone();
        let label = prompt.clone();
        let button = authorize.clone();
        session.connect_local("request", false, move |values| {
            label.set_text(values[1].get::<&str>().unwrap_or_default());
            field.set_visibility(values[2].get::<bool>().unwrap_or(false));
            field.set_sensitive(true);
            button.set_sensitive(true);
            field.grab_focus();
            None
        });
        for signal in ["show-error", "show-info"] {
            let label = message.clone();
            session.connect_local(signal, false, move |values| {
                label.set_text(values[1].get::<&str>().unwrap_or_default());
                label.set_visible(true);
                None
            });
        }
        let pending = Rc::downgrade(&active);
        session.connect_local("completed", false, move |values| {
            if let Some(active) = pending.upgrade() {
                let finished = values[0].get::<glib::Object>().unwrap();
                let mut state = active.borrow_mut();
                if state.session.as_ref() == Some(&finished) {
                    let success = values[1].get::<bool>().unwrap_or(false);
                    state.finish((!success).then_some((FAILED, "Authentication failed")));
                }
            }
            None
        });
        active.borrow_mut().session = Some(session.clone());
        unsafe { (polkit.initiate)(session.to_glib_none().0) };
    };

    parent.present();
    active.borrow_mut().dialog = Some(dialog.clone());
    dialog.present();
    start(&accounts);
    accounts.connect_selected_notify(start);
}

pub(super) fn report_registration(result: &Result<Agent, String>) -> Result<(), String> {
    result.as_ref().map(|_| ()).map_err(|err| {
        crate::core::log::log("WARN", &format!("Polkit: {err}"));
        trf(
            "Could not open the administrator prompt: {error}",
            &[("error", err.clone())],
        )
    })
}
