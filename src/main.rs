use std::io::IsTerminal as _; // for stdout().is_terminal()
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Local};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Terminal;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::interval;

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
struct ApiResponse {
    // PodcastIndex sometimes returns `status` as a boolean (true/false) or as a string.
    // Use a flexible type so deserialization doesn't fail on type variation.
    #[serde(default)]
    status: Option<Value>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    max: Option<u32>,
    #[serde(default)]
    feeds: Vec<Feed>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
struct Feed {
    id: Option<u64>,
    status: Option<String>,
    url: Option<String>,
    title: Option<String>,
    description: Option<String>,
    link: Option<String>,
    #[serde(rename = "timeAdded")]
    time_added: Option<i64>,
    #[serde(rename = "contentHash")]
    content_hash: Option<String>,
    language: Option<String>,
    image: Option<String>,
    #[serde(rename = "itunesId")]
    itunes_id: Option<u64>,
    source: Option<u32>,
}

#[derive(Debug, Clone)]
enum DataSource {
    Api,
    Empty,
}

#[derive(Debug, Clone)]
struct AppState {
    feeds: Vec<Feed>,
    scroll: usize,
    last_updated: Option<DateTime<Local>>,
    source: DataSource,
    status_msg: String,
}

impl AppState {
    fn new() -> Self {
        Self {
            feeds: Vec::new(),
            scroll: 0,
            last_updated: None,
            source: DataSource::Empty,
            status_msg: String::from("Press q to quit. Fetching…"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Ensure we're running in a real terminal (TTY). Many IDE "Run" consoles are not TTYs
    // and Ratatui won't be able to draw there, which looks like a blank window.
    if !std::io::stdout().is_terminal() {
        eprintln!(
            "This application must be run in a real terminal (TTY).\n\
             Please run from Windows Terminal, PowerShell, or cmd.exe.\n\
             Example:\n\
               cargo run\n"
        );
        return Ok(());
    }

    // Setup terminal UI
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    // Clear once so the first draw is visible even if previous output existed
    let _ = terminal.clear();

    let (tx, mut rx) = mpsc::unbounded_channel::<AppState>();
    let mut app = AppState::new();

    // Spawn background task to periodically fetch
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60));
        // First immediate fetch without waiting 60s
        if let Err(e) = fetch_and_send(tx_clone.clone()).await {
            let mut st = AppState::new();
            st.status_msg = format!("Initial fetch failed: {}", e);
            let _ = tx_clone.send(st);
        }
        loop {
            ticker.tick().await;
            if let Err(e) = fetch_and_send(tx_clone.clone()).await {
                let mut st = AppState::new();
                st.status_msg = format!("Fetch failed: {}", e);
                let _ = tx_clone.send(st);
            }
        }
    });

    // UI loop
    let mut ui_tick = interval(Duration::from_millis(200));
    loop {
        // Determine current viewport rows for the list area so we can clamp scrolling.
        // This mirrors the same layout used in the draw pass below.
        let term_size = terminal.size()?;
        let term_rect = Rect::new(0, 0, term_size.width, term_size.height);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),        // list
                Constraint::Length(3),     // status (taller)
            ])
            .split(term_rect);
        // Account for list block borders (top+bottom) when computing visible rows.
        let viewport_rows: usize = chunks[0].height.saturating_sub(2) as usize;
        let viewport_rows = viewport_rows.max(1);

        // Apply incoming state updates
        while let Ok(new_state) = rx.try_recv() {
            // Merge: replace data and status, keep scroll if possible
            let scroll = app.scroll;
            app = new_state;
            // Clamp preserved scroll so the last item remains visible when list shrinks.
            let max_scroll = app
                .feeds
                .len()
                .saturating_sub(viewport_rows);
            app.scroll = scroll.min(max_scroll);
        }

        // Also clamp scroll on window resizes (no new state), to keep last item visible.
        let max_scroll_now = app
            .feeds
            .len()
            .saturating_sub(viewport_rows);
        if app.scroll > max_scroll_now {
            app.scroll = max_scroll_now;
        }

        terminal.draw(|f| {
            let size = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1), // list
                    Constraint::Length(3), // status (taller)
                ])
                .split(size);

            // Render scrolling by using a viewport via Paragraph/List with offset isn't native; we can slice
            let visible = if app.scroll < app.feeds.len() {
                &app.feeds[app.scroll..]
            } else {
                &[]
            };
            let vis_items: Vec<ListItem> = visible
                .iter()
                .map(|feed| {
                    let title = feed.title.clone().unwrap_or_else(|| "<untitled>".into());
                    let lang = feed.language.clone().unwrap_or_default();
                    let link = feed.link.clone().or(feed.url.clone()).unwrap_or_default();
                    let line = Line::from(vec![
                        Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(" "),
                        Span::styled(
                            format!("({})", if lang.is_empty() { "n/a" } else { &lang }),
                            Style::default().fg(Color::Gray),
                        ),
                        Span::raw(" — "),
                        Span::styled(link, Style::default().fg(Color::Cyan)),
                    ]);
                    ListItem::new(line)
                })
                .collect();

            let list = List::new(vis_items)
                .block(Block::default().borders(Borders::ALL).title("Recent New Feeds"));
            f.render_widget(list, chunks[0]);

            let updated = app
                .last_updated
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "n/a".into());
            let src = match app.source {
                DataSource::Api => "API",
                DataSource::Empty => "-",
            };
            let status_text = vec![
                Line::from(vec![
                    Span::raw("q: quit  r: refresh  ↑/↓: scroll  PgUp/PgDn/Home/End: nav  "),
                    Span::styled(
                        format!("Updated: {}  Source: {}  ", updated, src),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(&app.status_msg),
                ]),
            ];
            let status = Paragraph::new(status_text)
                .block(
                    Block::default().borders(Borders::ALL)
                        .title("Status")
                );
            f.render_widget(status, chunks[1]);
        })?;

        // Input handling with non-blocking poll
        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('r') => {
                            // Trigger a manual refresh in the background
                            app.status_msg = String::from("Refreshing…");
                            let tx2 = tx.clone();
                            tokio::spawn(async move {
                                let _ = fetch_and_send(tx2).await;
                            });
                        }
                        KeyCode::Up => {
                            if app.scroll > 0 {
                                app.scroll -= 1;
                            }
                        }
                        KeyCode::Down => {
                            let max_scroll = app
                                .feeds
                                .len()
                                .saturating_sub(viewport_rows);
                            if app.scroll < max_scroll {
                                app.scroll += 1;
                            }
                        }
                        KeyCode::PageUp => {
                            let step = 10.min(app.scroll);
                            app.scroll -= step;
                        }
                        KeyCode::PageDown => {
                            let step = 10usize;
                            let max_scroll = app
                                .feeds
                                .len()
                                .saturating_sub(viewport_rows);
                            app.scroll = (app.scroll + step).min(max_scroll);
                        }
                        KeyCode::Home => app.scroll = 0,
                        KeyCode::End => {
                            let max_scroll = app
                                .feeds
                                .len()
                                .saturating_sub(viewport_rows);
                            app.scroll = max_scroll;
                        }
                        _ => {}
                    }
                }
            }
        }

        ui_tick.tick().await;
    }

    // Restore terminal
    disable_raw_mode()?;
    let mut stdout = std::io::stdout();
    stdout.execute(LeaveAlternateScreen)?;
    Ok(())
}

async fn fetch_and_send(tx: mpsc::UnboundedSender<AppState>) -> Result<()> {
    let (feeds, source) = match fetch_from_api().await {
        Ok(feeds) => (feeds, DataSource::Api),
        Err(e) => return Err(e),
    };

    let mut st = AppState::new();
    st.feeds = feeds;
    st.last_updated = Some(Local::now());
    st.source = source;
    st.status_msg = String::from("OK");
    tx.send(st).map_err(|_| anyhow::anyhow!("failed to send state"))?;
    Ok(())
}

async fn fetch_from_api() -> Result<Vec<Feed>> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.podcastindex.org/api/1.0/recent/newfeeds")
        .header("User-Agent", "pimonitor/0.1")
        .send()
        .await?
        .error_for_status()?;

    let api: ApiResponse = resp.json().await?;
    Ok(api.feeds)
}
