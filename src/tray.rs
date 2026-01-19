use crossbeam_channel::Sender;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIconBuilder, TrayIconEvent};

use crate::ControlCommand;

fn set_control_flow(control_flow: &mut ControlFlow) {
    *control_flow = ControlFlow::Wait;
}

#[derive(Debug, Clone)]
enum UserEvent {
    TrayIconEvent(tray_icon::TrayIconEvent),
    MenuEvent(tray_icon::menu::MenuEvent),
}

fn forward_tray_event(
    proxy: EventLoopProxy<UserEvent>,
) -> impl Fn(tray_icon::TrayIconEvent) + Send + Sync + 'static {
    move |event| {
        let _ = proxy.send_event(UserEvent::TrayIconEvent(event));
    }
}

fn forward_menu_event(
    proxy: EventLoopProxy<UserEvent>,
) -> impl Fn(tray_icon::menu::MenuEvent) + Send + Sync + 'static {
    move |event| {
        let _ = proxy.send_event(UserEvent::MenuEvent(event));
    }
}

pub fn run_tray(command_tx: Sender<ControlCommand>) -> ! {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(forward_tray_event(proxy.clone())));
    MenuEvent::set_event_handler(Some(forward_menu_event(proxy)));

    let open_config = MenuItem::new("Open Config", true, None);
    let reload_config = MenuItem::new("Reload Config", true, None);
    let quit = MenuItem::new("Exit", true, None);

    let tray_menu = Menu::new();
    tray_menu
        .append_items(&[
            &open_config,
            &reload_config,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .expect("failed to build tray menu");

    let mut tray_icon = None;

    event_loop.run(move |event, _, control_flow| {
        set_control_flow(control_flow);

        match event {
            Event::NewEvents(StartCause::Init) => {
                let icon = default_icon();
                tray_icon = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(tray_menu.clone()))
                        .with_tooltip("EasyPassword")
                        .with_icon(icon)
                        .build()
                        .expect("failed to create tray icon"),
                );

                #[cfg(target_os = "macos")]
                unsafe {
                    use objc2_core_foundation::{CFRunLoopGetMain, CFRunLoopWakeUp};
                    let rl = CFRunLoopGetMain().unwrap();
                    CFRunLoopWakeUp(&rl);
                }
            }
            Event::UserEvent(UserEvent::MenuEvent(event)) => {
                if event.id == open_config.id() {
                    let _ = command_tx.send(ControlCommand::OpenConfig);
                } else if event.id == reload_config.id() {
                    let _ = command_tx.send(ControlCommand::ReloadConfig);
                } else if event.id == quit.id() {
                    let _ = command_tx.send(ControlCommand::Exit);
                    tray_icon.take();
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::TrayIconEvent(_event)) => {}
            _ => {}
        }
    });
}

fn default_icon() -> tray_icon::Icon {
    const RGBA: &[u8] = include_bytes!("../assets/icon-tray-32.rgba");
    tray_icon::Icon::from_rgba(RGBA.to_vec(), 32, 32).expect("invalid tray icon")
}
