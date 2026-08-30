// === IMPORTS ===
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};
use std::{
    env, fs,
    io::{self, stdout, Read},
    path::PathBuf,
    time,
};
use open;
use chrono::DateTime;

// === MODELS ===
#[derive(Debug)]
pub struct DirItem {
    pub name: String,
    pub is_dir: bool,
    pub file_size: u64,
    pub modified: Option<time::SystemTime>,
}

impl DirItem {
    pub fn format_size(&self) -> String {
        if self.is_dir {
            return String::from("DIR");
        }
        
        if self.file_size < 1024 {
            format!("{} bytes", self.file_size)
        } else if self.file_size < 1024 * 1024 {
            format!("{} kb", self.file_size / 1024)
        } else if self.file_size < 1024 * 1024 * 1024 {
            format!("{:.1} mb", self.file_size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.2} gb", self.file_size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
    pub fn format_modified(&self) -> String {
        match self.modified {
            Some(sys_time) => {
                // Convert standard SystemTime into a Chrono Local time
                let datetime: DateTime<chrono::Local> = sys_time.into();
                // Format it nicely as YYYY-MM-DD HH:MM
                datetime.format("%Y-%m-%d %H:%M").to_string()
            }
            _ => String::from("Undefined"),
        }
    }
}

#[derive(PartialEq)]
pub enum AppMode {
    Normal,
    ConfirmDelete,
    Renaming,
}

// === APP STATE ===
pub struct App {
    pub current_path: PathBuf,
    pub items: Vec<DirItem>,
    pub cursor_position: usize,
    pub mode: AppMode,
    pub clipboard: Option<std::path::PathBuf>,
    pub rename_buffer: String,
    pub should_force_redraw: bool,
    pub show_hidden: bool,
}

impl App {
    // === INITIALIZATION ===
    pub fn new() -> Self {
        let starting_path = env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

        Self {
            mode: AppMode::Normal,
            current_path: starting_path,
            items: Vec::new(),
            cursor_position: 0,
            clipboard: None,
            rename_buffer: String::new(),
            should_force_redraw: false,
            show_hidden: false,
        }
    }

    pub fn populate_files(&mut self) {
        self.items.clear();
        self.cursor_position = 0;

        let entries = match fs::read_dir(&self.current_path) {
            Ok(iterator) => iterator,
            Err(_) => return,
        };
        for entry in entries {
            if let Ok(file) = entry {
                let path = file.path();
                let name = file.file_name().to_string_lossy().into_owned();
                if !self.show_hidden && name.starts_with('.') {
                    continue 
                } else {

                    let is_dir = path.is_dir();
                    let (file_size, modified) = if let Ok(metadata) = file.metadata() {
                        (metadata.len(), metadata.modified().ok())
                    } else {
                        (0, None)
                    };
                    self.items.push(DirItem {name, is_dir, file_size, modified});
                }
            }
        }
        self.items.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        
        if self.cursor_position >= self.items.len() && !self.items.is_empty() {
            self.cursor_position = self.items.len() - 1;
        } else if self.items.is_empty() {
            self.cursor_position = 0;
        }

    }

    // === NAVIGATION LOGIC ===
    pub fn cursor_up(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn cursor_down(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.cursor_position < self.items.len() - 1 {
            self.cursor_position += 1;
        }
    }

    pub fn enter_selected(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let selected_item: &DirItem = &self.items[self.cursor_position];

        if selected_item.is_dir {
            self.current_path.push(&selected_item.name);
            self.populate_files();
        } else {
            if let Some(path) = self.get_selected_path() {
                if is_text_file(&path) {
                    self.open_in_editor();
                } else {
                    let _ = open::that(path);
                }
            }
        }
    }    

    pub fn go_up(&mut self) {
        if self.current_path.pop() {
            self.populate_files();
        }
    }

    // === STATE HELPERS ===
    pub fn get_selected_name(&self) -> Option<String> {
        if self.items.is_empty() {
            return None; 
        }
        Some(self.items[self.cursor_position].name.clone())
    }

    pub fn get_selected_path(&self) -> Option<std::path::PathBuf> {
        if self.items.is_empty() {
            return None;
        }
        let file_name = &self.items[self.cursor_position].name;
        Some(self.current_path.join(file_name))
    }

    // === FILE OPERATIONS ===
    pub fn delete_item(&mut self) {
        if let Some(path) = self.get_selected_path() {
            if path.is_file() {
                let _ = std::fs::remove_file(&path);
            } else if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            }
            self.populate_files();

            if self.cursor_position >= self.items.len() && !self.items.is_empty() {
                self.cursor_position = self.items.len() - 1;
            }
        }
    }

    pub fn copy_item(&mut self) {
        if let Some(full_path) = self.get_selected_path() {
            self.clipboard = Some(full_path);
        }
    }

    pub fn paste_item(&mut self) {
        if let Some(source_path) = &self.clipboard {
            if let Some(file_name) = source_path.file_name() {
                let dest_path = self.current_path.join(file_name);

                if source_path.is_file() {
                    let _ = std::fs::copy(source_path, dest_path);
                    self.populate_files();
                }
            }
        }
    }
    pub fn rename_item(&mut self) {
        if let Some(old_path) = self.get_selected_path() {
            if !self.rename_buffer.is_empty() {
                let new_path = self.current_path.join(&self.rename_buffer);
                let _ = std::fs::rename(old_path, new_path);
                self.populate_files();
            }
        }
        self.rename_buffer.clear();
        self.mode = AppMode::Normal
    }

    pub fn open_in_editor(&mut self) {
        if let Some(path) = self.get_selected_path() {
            if path.is_file() {
                let _ = (stdout(), LeaveAlternateScreen);
                let _ = disable_raw_mode();

                let mut child = std::process::Command::new("nvim").arg(&path)
                    .spawn().expect("Failed to launch nvim");

                let _ = child.wait();

                let _ = enable_raw_mode();
                let _ = execute!(stdout(), EnterAlternateScreen);

                self.should_force_redraw = true;
            }
        }
    }


    // === UI & RENDERING ===
    pub fn ui(&self, frame: &mut Frame) {
        let ui_items: Vec<ListItem> = self.items.iter().map(|item| {
            let prefix = if item.is_dir { "📁 " } else { "📄 " };
            ListItem::new(format!(
                "{}{} {:<50} {:>30}   {}", 
                prefix, 
                "", 
                item.name, 
                item.format_size(), 
                item.format_modified()
            ))
        }).collect();

        let list = List::new(ui_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", self.current_path.display()))
            )
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        let mut list_state = ListState::default();
        list_state.select(Some(self.cursor_position));

        frame.render_stateful_widget(list, frame.area(), &mut list_state);

        if let AppMode::ConfirmDelete = self.mode {
            let target_name = self.get_selected_name().unwrap_or_else(|| String::from("this item"));
            let popup_area = centered_rect(60, 20, frame.area());

            frame.render_widget(Clear, popup_area);

            let warning_box = Block::default()
                .title(format!(" Delete {} (Y/N)? ", target_name))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .style(Style::default().fg(Color::Red));

            frame.render_widget(warning_box, popup_area);
        }

        if let AppMode::Renaming = self.mode {
            let popup_area = centered_rect(60, 3, frame.area());
            frame.render_widget(Clear, popup_area);
        
            let input_box = ratatui::widgets::Paragraph::new(self.rename_buffer.as_str()).block(
                Block::default().title("Rename").borders(Borders::ALL).border_style(Style::default().
                    fg(Color::Yellow)
                ));
            frame.render_widget(input_box, popup_area);
        }
    }

    // === EVENT LOOP ===
    pub fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;

        let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;

        loop {

            if self.should_force_redraw {
                terminal.clear()?;
                self.should_force_redraw = false;
            }

            terminal.draw(|f| self.ui(f))?;

            if let Event::Key(key) = event::read()? {
                match self.mode {
                    AppMode::Normal => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => break,
                        KeyCode::Up => self.cursor_up(),
                        KeyCode::Down => self.cursor_down(),
                        KeyCode::Enter => self.enter_selected(),
                        KeyCode::Backspace => self.go_up(),
                        KeyCode::Char('d') => {
                            if self.get_selected_name().is_some() {
                                self.mode = AppMode::ConfirmDelete;
                            }
                        },
                        KeyCode::Char('-') => {
                            self.show_hidden = if self.show_hidden {false} else {true};
                            self.populate_files();
                        },
                        KeyCode::Char('r') => { // Added the trigger for renaming!
                            if let Some(name) = self.get_selected_name() {
                                self.rename_buffer = name;
                                self.mode = AppMode::Renaming;
                            }
                        },
                        KeyCode::Char('c') => self.copy_item(),
                        KeyCode::Char('v') => self.paste_item(),
                        _ => {}
                    },
                    AppMode::ConfirmDelete => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            self.delete_item();
                            self.mode = AppMode::Normal;
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            self.mode = AppMode::Normal;
                        }
                        _ => {}
                    },
                    AppMode::Renaming => match key.code{
                        KeyCode::Char(c) => self.rename_buffer.push(c),
                        KeyCode::Backspace => {self.rename_buffer.pop();},
                        KeyCode::Enter => self.rename_item(),
                        KeyCode::Esc => {
                            self.rename_buffer.clear();
                            self.mode = AppMode::Normal;
                        }
                    _ => {}
                    }
                }
            }
        }
        execute!(stdout(), LeaveAlternateScreen)?;
        disable_raw_mode()?;

        Ok(())
    }
}

// === UTILITIES ===
fn centered_rect(percent_x: u16, fixed_y: u16, r: Rect) -> Rect {
    // Slice vertically using Min(0) to center it, but strictly enforce the fixed_y height
    let vertical_split = Layout::default() // Added Read heredefault()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),          // Top margin
            Constraint::Length(fixed_y), // The exact height of the popup in lines
            Constraint::Min(0),          // Bottom margin
        ])
        .split(r);

    // Slice horizontally using percentages just like the other popup
    let horizontal_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical_split[1]);

    horizontal_split[1]
}

fn is_text_file(path: &std::path::Path) -> bool {
    if let Ok(mut file) = std::fs::File::open(path) {
    let mut buffer = [0u8; 512];
    
    if let Ok(bytes_read) = file.read(&mut buffer) {
            if bytes_read == 0 {
                return true;
            }
            
            return !buffer.iter().take(bytes_read).any(|&byte| byte == 0);        }
    }
    false
}

// === MAIN ===
fn main() -> io::Result<()> {
    let mut app = App::new();
    app.populate_files();
    app.run()?;
    Ok(())
}
