// tui — Interfaz de terminal con dos modos de operación:
//
//   Modo Ratatui  (hay TTY real): pantalla alternativa con zona de log y prompt.
//   Modo Plain    (pipe / test) : println! a stdout y lectura de stdin en hilo aparte.
//
// El resto del código sólo usa dos operaciones:
//
//   tui.print(msg)      — muestra una línea en la salida
//   tui.read_line()     — devuelve Some(línea) al pulsar Enter, None si no hay entrada

use crossterm::{
    cursor::{EnableBlinking, SetCursorStyle},
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::Paragraph,
    Terminal,
};
use std::io::{self, BufRead};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const MAX_LOG_BYTES: usize = 65536;
const MAX_HISTORY: usize = 100;

// =============================================================================
// Tipo público
// =============================================================================

pub struct Tui {
    inner: Inner,
}

// =============================================================================
// Implementación interna
// =============================================================================

enum Inner {
    Ratatui {
        terminal: Terminal<CrosstermBackend<io::Stdout>>,
        log: String,
        input: String,
        history: [String; MAX_HISTORY],
        history_len: usize,
        history_pos: Option<usize>,
    },
    Plain {
        stdin_rx: mpsc::Receiver<String>,
    },
}

impl Tui {
    pub fn new() -> Self {
        if terminal::enable_raw_mode().is_ok() {
            let _ = execute!(io::stdout(), EnterAlternateScreen, EnableBlinking, SetCursorStyle::BlinkingBar);
            if let Ok(t) = Terminal::new(CrosstermBackend::new(io::stdout())) {
                let mut tui = Self {
                    inner: Inner::Ratatui {
                        terminal: t,
                        log: String::new(),
                        input: String::new(),
                        history: std::array::from_fn(|_| String::new()),
                        history_len: 0,
                        history_pos: None,
                    },
                };
                tui.draw();
                return tui;
            }
            let _ = terminal::disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in io::stdin().lock().lines().flatten() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self { inner: Inner::Plain { stdin_rx: rx } }
    }

    /// Muestra `msg` en la zona de log (Ratatui) o en stdout (Plain).
    pub fn print(&mut self, msg: &str) {
        if let Inner::Ratatui { log, .. } = &mut self.inner {
            if !log.is_empty() {
                log.push('\n');
            }
            log.push_str(msg);
            // Recortar si supera el límite
            if log.len() > MAX_LOG_BYTES {
                let excess = log.len() - MAX_LOG_BYTES;
                let bytes = log.as_bytes();
                let mut trim_to = excess;
                while trim_to < log.len() && bytes[trim_to] != b'\n' {
                    trim_to += 1;
                }
                if trim_to < log.len() {
                    trim_to += 1; // saltar el '\n'
                }
                log.replace_range(..trim_to, "");
            }
        } else {
            println!("{msg}");
        }
        self.draw();
    }

    /// Poll ~50 ms. Devuelve `Some(línea)` al pulsar Enter, `None` si no hay entrada.
    /// Ctrl-C devuelve `Some("quit")`.
    pub fn read_line(&mut self) -> Option<String> {
        match &mut self.inner {
            Inner::Plain { stdin_rx } => {
                thread::sleep(Duration::from_millis(50));
                match stdin_rx.try_recv() {
                    Ok(line) => Some(line),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => None,
                }
            }
            Inner::Ratatui { .. } => self.poll_ratatui(),
        }
    }

    // -------------------------------------------------------------------------
    // Privado
    // -------------------------------------------------------------------------

    fn poll_ratatui(&mut self) -> Option<String> {
        if !event::poll(Duration::from_millis(50)).unwrap_or(false) {
            return None;
        }
        let key = match event::read().ok()? {
            Event::Key(k) => k,
            _ => return None,
        };

        let result = match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some("quit".to_string())
            }
            KeyCode::Enter => {
                if let Inner::Ratatui { input, history, history_len, history_pos, .. } = &mut self.inner {
                    let line = input.trim().to_string();
                    input.clear();
                    *history_pos = None;
                    if !line.is_empty() && *history_len < MAX_HISTORY {
                        history[*history_len] = line.clone();
                        *history_len += 1;
                    }
                    Some(line)
                } else {
                    None
                }
            }
            KeyCode::Up => {
                if let Inner::Ratatui { input, history, history_len, history_pos, .. } = &mut self.inner {
                    if *history_len > 0 {
                        let new_pos = match *history_pos {
                            None => *history_len - 1,
                            Some(0) => 0,
                            Some(p) => p - 1,
                        };
                        *history_pos = Some(new_pos);
                        *input = history[new_pos].clone();
                    }
                }
                None
            }
            KeyCode::Down => {
                if let Inner::Ratatui { input, history, history_len, history_pos, .. } = &mut self.inner {
                    match *history_pos {
                        None => {}
                        Some(p) if p + 1 >= *history_len => {
                            *history_pos = None;
                            input.clear();
                        }
                        Some(p) => {
                            *history_pos = Some(p + 1);
                            *input = history[p + 1].clone();
                        }
                    }
                }
                None
            }
            KeyCode::Char(c) => {
                if let Inner::Ratatui { input, history_pos, .. } = &mut self.inner {
                    *history_pos = None;
                    input.push(c);
                }
                None
            }
            KeyCode::Backspace => {
                if let Inner::Ratatui { input, .. } = &mut self.inner {
                    input.pop();
                }
                None
            }
            _ => None,
        };

        self.draw();
        result
    }

    fn draw(&mut self) {
        if let Inner::Ratatui { terminal, log, input, .. } = &mut self.inner {
            let input_snap = input.clone();

            // Determinar altura disponible para el log
            let total_height = terminal.size().map(|s| s.height as usize).unwrap_or(25);
            let log_height = if total_height > 1 { total_height - 1 } else { 1 };

            // Contar líneas totales en el log
            let mut total_lines = 0usize;
            let log_bytes = log.as_bytes();
            if !log.is_empty() {
                total_lines = 1;
                for bi in 0..log_bytes.len() {
                    if log_bytes[bi] == b'\n' {
                        total_lines += 1;
                    }
                }
            }

            // Encontrar la posición de inicio para mostrar las últimas log_height líneas
            let skip = total_lines.saturating_sub(log_height);
            let start_pos = if skip == 0 {
                0
            } else {
                let mut newlines_seen = 0usize;
                let mut pos = 0usize;
                while pos < log_bytes.len() {
                    if log_bytes[pos] == b'\n' {
                        newlines_seen += 1;
                        if newlines_seen == skip {
                            pos += 1;
                            break;
                        }
                    }
                    pos += 1;
                }
                pos
            };

            let visible = log[start_pos..].to_string();

            terminal
                .draw(|f| {
                    let area = f.size();
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(1), Constraint::Length(1)])
                        .split(area);

                    f.render_widget(Paragraph::new(visible), chunks[0]);
                    f.render_widget(Paragraph::new(format!("> {input_snap}")), chunks[1]);

                    let cursor_x = chunks[1].x + 2 + input_snap.len() as u16;
                    let cursor_y = chunks[1].y;
                    f.set_cursor(cursor_x, cursor_y);
                })
                .ok();
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        if let Inner::Ratatui { .. } = &self.inner {
            let _ = terminal::disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
    }
}
