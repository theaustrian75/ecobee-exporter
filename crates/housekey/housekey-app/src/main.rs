fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter("housekey=debug,housekey_app=debug")
        .init();

    iced::application("Housekey", App::update, App::view).run()
}

#[derive(Default)]
struct App;

#[derive(Debug, Clone)]
enum Message {}

impl App {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> iced::Element<'_, Message> {
        iced::widget::text("housekey").into()
    }
}
