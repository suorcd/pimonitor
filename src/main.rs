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
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
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
    selected: usize,
    last_updated: Option<DateTime<Local>>,
    source: DataSource,
    status_msg: String,
    show_popup: bool,
}

impl AppState {
    fn new() -> Self {
        Self {
            feeds: Vec::new(),
            scroll: 0,
            selected: 0,
            last_updated: None,
            source: DataSource::Empty,
            status_msg: String::from("Press q to quit. Fetching…"),
            show_popup: false,
        }
    }
}

// Helper to create a centered rectangle with a percentage of the given area
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1]);
    horizontal[1]
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
        // Each feed is rendered as TWO lines (image on first line; id, title, url on second),
        // so compute the number of visible ITEMS (feeds) as half of visible rows.
        let viewport_rows_lines: usize = chunks[0].height.saturating_sub(2) as usize;
        let viewport_items: usize = (viewport_rows_lines / 2).max(1);

        // Apply incoming state updates
        while let Ok(new_state) = rx.try_recv() {
            // Merge: replace data and status, keep scroll if possible
            let scroll = app.scroll;
            let selected = app.selected;
            app = new_state;
            // Clamp preserved scroll so the last item remains visible when list shrinks.
            let max_scroll = app
                .feeds
                .len()
                .saturating_sub(viewport_items);
            app.scroll = scroll.min(max_scroll);
            // Clamp selected within bounds
            if app.feeds.is_empty() {
                app.selected = 0;
            } else {
                app.selected = selected.min(app.feeds.len() - 1);
            }
        }

        // Also clamp scroll on window resizes (no new state), to keep last item visible.
        let max_scroll_now = app
            .feeds
            .len()
            .saturating_sub(viewport_items);
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
                    let lang_str = feed.language.clone().unwrap_or_else(|| "n/a".into());
                    let link = feed.link.clone().unwrap_or_else(|| "<no link>".into());
                    let url = feed.url.clone().unwrap_or_default();
                    let image = feed.image.clone().unwrap_or_default();
                    let id_str = feed
                        .id
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "?".into());

                    // First line: [id]. [title] ([language]) - [url]
                    let line1 = Line::from(vec![
                        Span::styled(format!("{}.", id_str), Style::default().fg(Color::Yellow)),
                        Span::raw(" "),
                        Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format!(" ({})", lang_str), Style::default().fg(Color::LightMagenta)),
                        Span::raw(" - "),
                        Span::styled(url, Style::default().fg(Color::Cyan)),
                    ]);

                    // Second line: link
                    let line2 = Line::from(vec![
                        Span::styled(format!("    {}.", link), Style::default().fg(Color::LightMagenta)),
                    ]);

                    ListItem::new(vec![line1, line2])
                })
                .collect();

            let mut list_state = ratatui::widgets::ListState::default();
            // Selected relative to current scroll viewport
            if !app.feeds.is_empty() && app.selected >= app.scroll {
                let rel = app.selected - app.scroll;
                list_state.select(Some(rel));
            } else {
                list_state.select(None);
            }

            let list = List::new(vis_items)
                .block(Block::default().borders(Borders::ALL).title("Recent New Feeds"))
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                .highlight_symbol("▶ ");
            f.render_stateful_widget(list, chunks[0], &mut list_state);

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
                    Span::raw("q: quit  r: refresh  ↑/↓: select  Enter: open  Esc: close  PgUp/PgDn/Home/End: nav  "),
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

            // If popup requested, draw it centered over everything
            if app.show_popup {
                let area = centered_rect(70, 60, size);
                let feed_opt = app.feeds.get(app.selected);
                if let Some(feed) = feed_opt {
                    let title = feed.title.clone().unwrap_or_else(|| "<untitled>".into());
                    let desc = feed.description.clone().unwrap_or_else(|| "<no description>".into());
                    let popup_text = vec![
                        Line::from(Span::styled(title.clone(), Style::default().add_modifier(Modifier::BOLD))),
                        Line::from(""),
                        Line::from(desc),
                        Line::from(""),
                        Line::from(Span::styled("Press Esc to close", Style::default().fg(Color::DarkGray))),
                    ];
                    let popup = Paragraph::new(popup_text)
                        .block(Block::default().title("Feed Details").borders(Borders::ALL))
                        .wrap(Wrap { trim: true });
                    // Clear the area first so the popup is readable
                    f.render_widget(Clear, area);
                    f.render_widget(popup, area);
                }
            }
        })?;

        // Input handling with non-blocking poll
        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Esc => {
                            if app.show_popup {
                                app.show_popup = false;
                            } else {
                                break;
                            }
                        }
                        KeyCode::Char('r') => {
                            // Trigger a manual refresh in the background
                            app.status_msg = String::from("Refreshing…");
                            let tx2 = tx.clone();
                            tokio::spawn(async move {
                                let _ = fetch_and_send(tx2).await;
                            });
                        }
                        KeyCode::Enter => {
                            if !app.feeds.is_empty() {
                                app.show_popup = true;
                            }
                        }
                        KeyCode::Up => {
                            if app.selected > 0 {
                                app.selected -= 1;
                                // Ensure selected remains visible
                                if app.selected < app.scroll {
                                    app.scroll = app.selected;
                                }
                            }
                        }
                        KeyCode::Down => {
                            if app.selected + 1 < app.feeds.len() {
                                app.selected += 1;
                                let bottom = app.scroll + viewport_items;
                                if app.selected >= bottom {
                                    app.scroll = app.selected + 1 - viewport_items;
                                }
                            }
                        }
                        KeyCode::PageUp => {
                            let step = viewport_items.max(1);
                            if app.selected >= step {
                                app.selected -= step;
                            } else {
                                app.selected = 0;
                            }
                            if app.selected < app.scroll {
                                app.scroll = app.selected;
                            }
                        }
                        KeyCode::PageDown => {
                            let step = viewport_items.max(1);
                            if !app.feeds.is_empty() {
                                let max_idx = app.feeds.len() - 1;
                                app.selected = (app.selected + step).min(max_idx);
                                let bottom = app.scroll + viewport_items;
                                if app.selected >= bottom {
                                    app.scroll = app.selected + 1 - viewport_items;
                                }
                            }
                        }
                        KeyCode::Home => {
                            app.selected = 0;
                            app.scroll = 0;
                        }
                        KeyCode::End => {
                            if !app.feeds.is_empty() {
                                app.selected = app.feeds.len() - 1;
                                let max_scroll = app
                                    .feeds
                                    .len()
                                    .saturating_sub(viewport_items);
                                app.scroll = max_scroll;
                            }
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
