// quick launcher: Super+S pops a search bar, type a filename or just hit
// enter to fall through to a web search. esc closes it back down.

use std::path::PathBuf;

use global_hotkey::hotkey::{Code, HotKey, Modifiers as HotKeyModifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use iced::futures::{SinkExt, Stream};
use iced::widget::{button, column, container, operation, row, scrollable, text, text_input};
use iced::{keyboard, window};
use iced::{
    Background, Border, Color, Element, Event, Length, Shadow, Subscription, Task, Theme,
};

const WINDOW_WIDTH: f32 = 640.0;
const WINDOW_HEIGHT: f32 = 440.0;
const MAX_RESULTS: usize = 12;
const MAX_SCAN: usize = 40_000;
const MAX_DEPTH: usize = 8;
const SEARCH_INPUT_ID: &str = "divina-search-input";

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .subscription(App::subscription)
        .theme(App::theme)
        .style(App::style)
        .title("Divina Search")
        .window(window::Settings {
            size: iced::Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            position: window::Position::Centered,
            resizable: false,
            decorations: false,
            visible: false,
            level: window::Level::AlwaysOnTop,
            ..window::Settings::default()
        })
        .run()
}

#[derive(Debug, Clone)]
struct SearchResult {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

struct App {
    window_id: Option<window::Id>,
    visible: bool,
    hotkey_registered: bool,
    query: String,
    results: Vec<SearchResult>,
    selected: usize,
    generation: u64,
    // has to stay alive - drop this and the hotkey unregisters itself
    _hotkey_manager: GlobalHotKeyManager,
}

#[derive(Debug, Clone)]
enum Message {
    WindowOpened(window::Id),
    QueryChanged(String),
    SearchFinished(u64, Vec<SearchResult>),
    Submit,
    MoveSelection(i32),
    ResultChosen(usize),
    HotKeyPressed,
    Hide,
}

impl App {
    fn new() -> Self {
        let manager =
            GlobalHotKeyManager::new().expect("failed to initialize global hotkey manager");

        // Super+S first choice. Windows sometimes eats it (search charm etc)
        // so we try a couple of backups before giving up.
        let candidates = [
            (HotKeyModifiers::SUPER, Code::KeyS, "Super+S"),
            (HotKeyModifiers::CONTROL | HotKeyModifiers::ALT, Code::KeyS, "Ctrl+Alt+S"),
            (HotKeyModifiers::CONTROL | HotKeyModifiers::SHIFT, Code::KeyS, "Ctrl+Shift+S"),
        ];

        let mut hotkey_registered = false;
        for (mods, code, label) in candidates {
            if manager.register(HotKey::new(Some(mods), code)).is_ok() {
                eprintln!("hotkey registered: {label}");
                hotkey_registered = true;
                break;
            }
        }

        if !hotkey_registered {
            eprintln!("couldn't grab any hotkey, they're all taken. opening visible instead");
        }

        Self {
            window_id: None,
            visible: false,
            hotkey_registered,
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            generation: 0,
            _hotkey_manager: manager,
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowOpened(id) => {
                self.window_id = Some(id);
                if self.hotkey_registered {
                    Task::none()
                } else {
                    self.show_window()
                }
            }
            Message::QueryChanged(query) => {
                self.selected = 0;
                // bump generation so a slow search from an older keystroke
                // can't clobber results from a newer one when it lands late
                self.generation += 1;
                let generation = self.generation;

                if query.trim().is_empty() {
                    self.query = query;
                    self.results.clear();
                    return Task::none();
                }

                self.query = query.clone();
                Task::perform(
                    async move { search_files(&query) },
                    move |results| Message::SearchFinished(generation, results),
                )
            }
            Message::SearchFinished(generation, results) => {
                if generation == self.generation {
                    self.results = results;
                    self.selected = 0;
                }
                Task::none()
            }
            Message::MoveSelection(delta) => {
                let total = self.total_entries() as i32;
                if total > 0 {
                    let current = self.selected as i32;
                    self.selected = (current + delta).rem_euclid(total) as usize;
                }
                Task::none()
            }
            Message::Submit => self.activate(self.selected),
            Message::ResultChosen(index) => self.activate(index),
            Message::HotKeyPressed => self.toggle_window(),
            Message::Hide => self.hide_window(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let input = text_input("Search files, or press Enter to search the web…", &self.query)
            .id(SEARCH_INPUT_ID)
            .on_input(Message::QueryChanged)
            .on_submit(Message::Submit)
            .size(20)
            .padding(16)
            .style(input_style);

        let body: Element<'_, Message> = if self.query.trim().is_empty() {
            container(
                text("Type to search your files, or the web.")
                    .size(14)
                    .color(Color::from_rgb8(120, 120, 120)),
            )
            .padding(20)
            .width(Length::Fill)
            .into()
        } else {
            let mut list = column![].spacing(2).padding(8);

            for (i, result) in self.results.iter().enumerate() {
                let icon = if result.is_dir { "📁" } else { "📄" };
                list = list.push(result_row(
                    icon,
                    result.name.clone(),
                    result.path.display().to_string(),
                    i == self.selected,
                    Message::ResultChosen(i),
                ));
            }

            let web_index = self.results.len();
            list = list.push(result_row(
                "🔎",
                format!("Search the web for \"{}\"", self.query.trim()),
                "Open in default browser".to_string(),
                web_index == self.selected,
                Message::ResultChosen(web_index),
            ));

            scrollable(list).height(Length::Fill).into()
        };

        container(column![input, body].spacing(0))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(app_style)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::open_events().map(Message::WindowOpened),
            iced::event::listen_with(handle_event),
            Subscription::run(hotkey_stream),
        ])
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn style(&self, _theme: &Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: Color::from_rgb8(0, 0, 0),
            text_color: Color::from_rgb8(230, 230, 230),
        }
    }

    fn total_entries(&self) -> usize {
        if self.query.trim().is_empty() {
            0
        } else {
            self.results.len() + 1
        }
    }

    fn activate(&mut self, index: usize) -> Task<Message> {
        if !self.query.trim().is_empty() {
            if index < self.results.len() {
                let _ = open::that_detached(&self.results[index].path);
            } else {
                let url = format!(
                    "https://www.google.com/search?q={}",
                    urlencoding::encode(self.query.trim())
                );
                let _ = open::that_detached(url);
            }
        }
        self.hide_window()
    }

    fn hide_window(&mut self) -> Task<Message> {
        self.query.clear();
        self.results.clear();
        self.selected = 0;
        self.generation += 1;
        self.visible = false;

        match self.window_id {
            Some(id) => window::set_mode(id, window::Mode::Hidden),
            None => Task::none(),
        }
    }

    fn toggle_window(&mut self) -> Task<Message> {
        if self.visible {
            self.hide_window()
        } else {
            self.show_window()
        }
    }

    fn show_window(&mut self) -> Task<Message> {
        let Some(id) = self.window_id else {
            return Task::none();
        };

        self.visible = true;
        Task::batch([
            window::set_mode(id, window::Mode::Windowed),
            window::gain_focus(id),
            operation::focus(SEARCH_INPUT_ID),
        ])
    }
}

fn handle_event(event: Event, _status: iced::event::Status, _window: window::Id) -> Option<Message> {
    if let Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = event {
        match key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => Some(Message::Hide),
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Some(Message::MoveSelection(-1)),
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Some(Message::MoveSelection(1)),
            _ => None,
        }
    } else {
        None
    }
}

fn hotkey_stream() -> impl Stream<Item = Message> {
    // global-hotkey hands us events on a plain crossbeam channel, so we
    // park a regular OS thread on recv() and forward each one into iced's
    // async world by hand. block_on is fine here, this thread does nothing else.
    iced::stream::channel(16, async move |sender| {
        std::thread::spawn(move || {
            let mut sender = sender;
            let receiver = GlobalHotKeyEvent::receiver();
            while let Ok(event) = receiver.recv() {
                if event.state() == HotKeyState::Pressed
                    && iced::futures::executor::block_on(sender.send(Message::HotKeyPressed))
                        .is_err()
                {
                    break;
                }
            }
        });
    })
}

fn result_row<'a>(
    icon: &'static str,
    title: String,
    subtitle: String,
    selected: bool,
    message: Message,
) -> Element<'a, Message> {
    let content = row![
        text(icon).size(18),
        column![
            text(title).size(15),
            text(subtitle).size(11).color(Color::from_rgb8(140, 140, 140)),
        ]
        .spacing(2),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);

    button(content)
        .on_press(message)
        .width(Length::Fill)
        .padding([10, 14])
        .style(move |theme, status| row_style(theme, status, selected))
        .into()
}

fn app_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(Color::from_rgb8(230, 230, 230)),
        background: Some(Background::Color(Color::from_rgb8(8, 8, 8))),
        border: Border {
            color: Color::from_rgb8(35, 35, 35),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn input_style(_theme: &Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(Color::from_rgb8(8, 8, 8)),
        border: Border {
            color: Color::from_rgb8(8, 8, 8),
            width: 0.0,
            radius: 0.0.into(),
        },
        icon: Color::from_rgb8(230, 230, 230),
        placeholder: Color::from_rgb8(110, 110, 110),
        value: Color::from_rgb8(235, 235, 235),
        selection: Color::from_rgb8(60, 90, 140),
    }
}

fn row_style(_theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let background = if selected {
        Color::from_rgb8(36, 36, 42)
    } else if matches!(status, button::Status::Hovered) {
        Color::from_rgb8(22, 22, 22)
    } else {
        Color::from_rgb8(8, 8, 8)
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::from_rgb8(230, 230, 230),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn search_files(query: &str) -> Vec<SearchResult> {
    // this runs on every keystroke, so bail out after MAX_SCAN entries
    // rather than crawling someone's entire Documents folder every time
    let query_lower = query.to_lowercase();
    let roots = search_roots();
    let mut matches: Vec<(u8, SearchResult)> = Vec::new();
    let mut scanned = 0usize;

    'roots: for root in roots {
        let walker = walkdir::WalkDir::new(&root)
            .max_depth(MAX_DEPTH)
            .into_iter()
            .filter_entry(|entry| !is_ignored(entry));

        for entry in walker {
            let Ok(entry) = entry else { continue };

            scanned += 1;
            if scanned > MAX_SCAN {
                break 'roots;
            }

            if entry.depth() == 0 {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            let name_lower = name.to_lowercase();

            if let Some(rank) = match_rank(&name_lower, &query_lower) {
                matches.push((
                    rank,
                    SearchResult {
                        name,
                        path: entry.path().to_path_buf(),
                        is_dir: entry.file_type().is_dir(),
                    },
                ));
            }
        }
    }

    matches.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.len().cmp(&b.1.name.len())));
    matches.into_iter().take(MAX_RESULTS).map(|(_, r)| r).collect()
}

fn match_rank(name_lower: &str, query_lower: &str) -> Option<u8> {
    if name_lower == query_lower {
        Some(0)
    } else if name_lower.starts_with(query_lower) {
        Some(1)
    } else if name_lower.contains(query_lower) {
        Some(2)
    } else {
        None
    }
}

fn is_ignored(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }

    // dotfiles and the usual junk directories nobody's searching for
    let name = entry.file_name().to_string_lossy();
    name.starts_with('.')
        || matches!(
            name.as_ref(),
            "node_modules"
                | "target"
                | "$RECYCLE.BIN"
                | "System Volume Information"
                | "AppData"
                | "Library"
        )
}

fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(dir) = dirs::desktop_dir() {
        roots.push(dir);
    }
    if let Some(dir) = dirs::document_dir() {
        roots.push(dir);
    }
    if let Some(dir) = dirs::download_dir() {
        roots.push(dir);
    }
    if let Some(dir) = dirs::picture_dir() {
        roots.push(dir);
    }
    if let Some(dir) = dirs::video_dir() {
        roots.push(dir);
    }

    if roots.is_empty() {
        if let Some(dir) = dirs::home_dir() {
            roots.push(dir);
        }
    }

    roots
}
