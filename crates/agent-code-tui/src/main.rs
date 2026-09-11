use std::io;

use agent_code_storage::SqliteTaskBoard;
use agent_code_tui::dashboard_text;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, widgets::Paragraph, Terminal};
use rusqlite::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database = std::env::args()
        .nth(1)
        .ok_or("usage: agent-code-tui <database>")?;
    let board = SqliteTaskBoard::open(Connection::open(database)?)?;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run(&mut terminal, &board);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result.map_err(Into::into)
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    board: &SqliteTaskBoard,
) -> Result<(), String> {
    loop {
        let text = dashboard_text(board)?;
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new(text), frame.area()))
            .map_err(|e| e.to_string())?;
        if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
            if key.code == KeyCode::Char('q') {
                return Ok(());
            }
        }
    }
}
