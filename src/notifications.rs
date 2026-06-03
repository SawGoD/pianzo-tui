use notify_rust::Notification;

pub fn playing(name: &str) {
    let _ = Notification::new()
        .summary("Сейчас играет")
        .body(name)
        .show();
}

pub fn stopped(name: &str) {
    let _ = Notification::new()
        .summary("Остановлено")
        .body(name)
        .show();
}
