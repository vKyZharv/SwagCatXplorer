// === IMPORTS ===
use crossterm::{
    event::{self, Event::self , KeyCode}, execute, terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use std::{
    env, fs,
    io::{self, stdout, Read},
    path::PathBuf,
    time,
};
use chrono::{DateTime};

use ratatui_image::{
    picker::Picker, 
};

pub struct KeyBinding {
    pub key : &'static str,
    pub description : &'static str,
}

pub enum PreviewData {
    Text(String),
    Image(ratatui_image::protocol::StatefulProtocol),
    Unsupported,
    None
}

const HELP_ITEMS: &[KeyBinding; 13] = &[
    KeyBinding { key: "↑/↓", description: "Navigate" },
    KeyBinding { key: "Enter", description: "Open/Dir" },
    KeyBinding { key: "Backspace", description: "Back" },
    KeyBinding { key: "n", description: "New" },
    KeyBinding { key: "r", description: "Rename" },
    KeyBinding { key: "d", description: "Delete" },
    KeyBinding { key: "c", description: "Copy" },
    KeyBinding { key: "v", description: "Paste" },
    KeyBinding { key: "]", description: "Hidden" },
    KeyBinding { key: "q", description: "Quit" },
    KeyBinding { key: "/", description: "Search"},
    KeyBinding { key: "h", description: "History"},
    KeyBinding { key: "s", description: "SortMethod"}
];



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
#[derive(Debug, PartialEq, Clone)]
pub enum SortMethod {
    Name,
    Size,
    DateModified,
}

#[derive(PartialEq)]
pub enum AppMode {
    Normal,
    ConfirmDelete,
    Renaming,
    Create,
    Search,
    History,
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
    pub create_buffer: String,
    pub search_buffer:String,
    pub status_message: Vec<String>,
    pub notification: Option<String>,
    pub sort_method: SortMethod,
    pub reverse: bool,
    pub preview: PreviewData,
    pub picker: Picker,

}

impl App {
    // === INITIALIZATION ===
    pub fn new() -> Self {
        let starting_path = env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

        Self {
            mode: AppMode::Normal,
            current_path: starting_path,
            items: Vec::new(),
            cursor_position: 0,
            clipboard: None,
            rename_buffer: String::new(),
            should_force_redraw: false,
            show_hidden: false,
            create_buffer: String::new(),
            search_buffer: String::new(),
            status_message: Vec::new(),
            notification: None,
            sort_method: SortMethod::Name,
            reverse: false,
            preview: PreviewData::None,
            picker,
        }
    }

    pub fn render_footer(&self, frame:&mut Frame, area:Rect) {
        let mid = HELP_ITEMS.len() / 2;
        let (top_row, bottom_row) = HELP_ITEMS.split_at(mid);

        let mut lines = Vec::new();

        for row_items in [top_row, bottom_row] {
            let mut spans = Vec::new();
            
            for (i, item) in row_items.iter().enumerate() {
                spans.push(text::Span::styled(
                    item.key,
                    Style::default().fg(Color::Yellow)
                ));

            spans.push(ratatui::text::Span::raw(format!("-> {}", item.description)));

            if i < HELP_ITEMS.len() - 1 {
                spans.push(text::Span::styled(
                        "   > ", 
                        Style::default().fg(Color::DarkGray)
               ));
            }    
        }
            lines.push(text::Line::from(spans));
        }
            let help_paragraph = Paragraph::new(lines)
                .block(
                Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
            ).alignment(Alignment::Center);

            frame.render_widget(help_paragraph, area);
    }

    pub fn populate_files(&mut self) {
        self.items.clear();
        self.cursor_position = 0;
        
        if !self.search_buffer.is_empty() {
            populate_search_results(
                &self.current_path,
                &self.search_buffer,
                self.show_hidden,
                &mut self.items
                );
        } else {
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
                }
                if !self.search_buffer.is_empty() {
                        if !name.to_lowercase().contains(&self.search_buffer.to_lowercase()) {
                            continue;
                        }
                }
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
        self.sort_files();
        self.fn_preview();
        
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
            self.fn_preview();
        }
    }

    pub fn cursor_down(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.cursor_position < self.items.len() - 1 {
            self.cursor_position += 1;
            self.fn_preview();
        }
    }

    pub fn enter_selected(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let selected_item: &DirItem = &self.items[self.cursor_position];

        if selected_item.is_dir {
            self.current_path.push(&selected_item.name);
            self.search_buffer.clear();
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
            self.search_buffer.clear();
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
    
    pub fn sort_files(&mut self){

        let method = self.sort_method.clone();

        self.items.sort_by(|a,b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| match method {

            SortMethod::Name => a.name.to_lowercase()
                .cmp(&b.name.to_lowercase()),
            SortMethod::Size => a.file_size.cmp(&b.file_size),
            SortMethod::DateModified => a.modified.cmp(&b.modified),
        }).then_with(|| a.name.to_lowercase()
        .cmp(&b.name.to_lowercase()))
        
        });
    }

    pub fn render_preview(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title("File Contents")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        match &mut self.preview { // <-- MATCH AS MUTABLE
            PreviewData::Text(text) => {
                let preview_box = Paragraph::new(text.as_str())
                    .block(block)
                    .wrap(ratatui::widgets::Wrap { trim: false });
                frame.render_widget(preview_box, area);
            }
            PreviewData::Image(protocol) => {
                let inner_area = block.inner(area);
                frame.render_widget(block, area); 
                
                // FIXED: Render using the v11 Stateful widget
                let image_widget = ratatui_image::StatefulImage::default();
                frame.render_stateful_widget(image_widget, inner_area, protocol);
            }            
            PreviewData::Unsupported => {
                let msg_box = Paragraph::new("-- Binary or Unsupported File --")
                    .block(block)
                    .alignment(Alignment::Center);
                frame.render_widget(msg_box, area);
            }
            PreviewData::None => {
                // Just render an empty bordered box for directories
                frame.render_widget(block, area); 
            }
        }
    }

    pub fn cycle_sort(&mut self) {
        self.sort_method = match self.sort_method {
            SortMethod::Name => SortMethod::Size,
            SortMethod::Size => SortMethod::DateModified,
            SortMethod::DateModified => SortMethod::Name,
        };
        self.sort_files();
        self.log_action(format!("Sorted by {:?}", self.sort_method));
    }

    pub fn get_selected_path(&self) -> Option<std::path::PathBuf> {
        if self.items.is_empty() {
            return None;
        }
        let file_name = &self.items[self.cursor_position].name;
        Some(self.current_path.join(file_name))
    }

    pub fn log_action(&mut self, msg: String) {
        self.notification = Some(msg.clone());
        self.status_message.push(msg);
    }

    // === FILE OPERATIONS ===
    
    pub fn show_history(&self, frame: &mut Frame) {
        if self.status_message.is_empty() {
            return;
        }

        let popup_area = right_panel_rect(80,frame.area());
        frame.render_widget(Clear,popup_area);

        let log_items: Vec<ListItem> = self.status_message
            .iter()
            .rev()
            .take(40)
            .map(|msg| ListItem::new(
                    msg.as_str()))
            .collect();

        let log_list = List::new(log_items)
            .block(
                Block::default()
                .title("History")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue))
                )
            .style(Style::default().fg(Color::LightGreen));
        frame.render_widget(log_list, popup_area);
    }
    
    pub fn fn_preview(&mut self) {
        if let Some(path) = self.get_selected_path() {
            if path.is_dir() {
                self.preview = PreviewData::None;
            } else if _is_image_file(&path) {
                if let Ok(dyn_image) = image::open(&path) {
                    // FIXED: Added the missing closing parenthesis
                    let protocol = self.picker.new_resize_protocol(dyn_image);
                    self.preview = PreviewData::Image(protocol);
                } else {
                    self.preview = PreviewData::Unsupported;
                }
            } else if is_text_file(&path) {
                if let Ok(mut file) = std::fs::File::open(&path) {
                    let mut buffer = vec![0u8; 2048];
                    if let Ok(bytes) = file.read(&mut buffer) {
                        // FIXED: Replaced Some(...) with PreviewData::Text(...)
                        self.preview = PreviewData::Text(
                            String::from_utf8_lossy(&buffer[0..bytes]).into_owned()
                        );
                    }
                }
            } else {
                self.preview = PreviewData::Unsupported;
            }
        } else {
            self.preview = PreviewData::None;
        }
    }

    pub fn delete_item(&mut self) {
        if let Some(path) = self.get_selected_path() {
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            if path.is_file() {
                let _ = std::fs::remove_file(&path);
                self.log_action(format!("File {} deleted!", name));
            } else if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
                self.log_action(format!("Folder {} deleted!", name));
            }
            self.populate_files();

            if self.cursor_position >= self.items.len() && !self.items.is_empty() {
                self.cursor_position = self.items.len() - 1;
            }
        }
    }

    pub fn copy_item(&mut self) {
        if let Some(full_path) = self.get_selected_path() {
            let name = full_path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            self.clipboard = Some(full_path);

            self.log_action(format!("Copied: {}", name));
        }
    }

    pub fn paste_item(&mut self) {
        if let Some(source_path) = &self.clipboard.clone() {
            if let Some(file_name) = source_path.file_name() {
                let dest_path = self.current_path.join(file_name);
                let name = file_name.to_string_lossy();

                if source_path.is_file() {
                    let _ = std::fs::copy(source_path, &dest_path);
                    self.populate_files();
                }

            self.log_action(format!("Pasted: {} at {}",name, dest_path.display()));
            }
        }
    }
    pub fn create(&mut self) {
            if self.create_buffer.is_empty() {
                self.mode = AppMode::Normal;
                return;
            }

            let target_path = self.current_path.join(&self.create_buffer);

            if self.create_buffer.ends_with('/') {
                let _ = std::fs::create_dir_all(&target_path);
                self.log_action(format!("New folder {} created", &self.create_buffer));
            } else {
                let _ = std::fs::File::create(&target_path);
                self.log_action(format!("New file {} created", &self.create_buffer));
            }

            self.mode = AppMode::Normal;
            self.populate_files();

            let search_name = self.create_buffer.trim_end_matches('/');
            if let Some(pos) = self.items.iter()
                .position(| i| i.name == search_name){
                self.cursor_position = pos;
            }
            self.create_buffer.clear();
        }
    
    pub fn rename_item(&mut self) {
        if let Some(old_path) = self.get_selected_path() {
            if !self.rename_buffer.is_empty() {
                let new_path = self.current_path.join(&self.rename_buffer);
                let old_name = old_path.clone()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let _ = std::fs::rename(old_path, new_path);
                self.populate_files();

                self.log_action(format!
                    ("File/Folder renamed from {} -> {}", old_name, &self.rename_buffer));
            }
        }
        self.rename_buffer.clear();
        self.mode = AppMode::Normal;
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
    pub fn ui(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0), 
                Constraint::Length(4),
            ])
            .split(frame.area());

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
                    .title(if self.search_buffer.is_empty() {
                        format!(" {} ", self.current_path.display())
                    } else {
                        format!("{} | Filter: {}", self.current_path.display(), self.search_buffer)
                    }
            ))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        let mut list_state = ListState::default();
        list_state.select(Some(self.cursor_position));

        let main_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(chunks[0]);
        
        frame.render_stateful_widget(list, main_columns[0], &mut list_state);
        self.render_preview(frame, main_columns[1]);

        self.render_footer(frame, chunks[1]);

        if let Some(msg) = &self.notification {
            let pop_up = top_right_rect(80, 3, frame.area());
            frame.render_widget(Clear, pop_up);
        

            let status_box = Paragraph::new(msg.as_str())
                .block(
                    Block::default()
                    .title("Status")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    )
                .style(Style::default().fg(Color::Cyan));
            frame.render_widget(status_box,pop_up);
        }
        
        if let AppMode::History = self.mode {
            self.show_history(frame);
        }


        if let AppMode::ConfirmDelete = self.mode {
            let target_name = self.get_selected_name().unwrap_or_else(|| String::from("this item"));
            let popup_area = centered_rect(10, 3, frame.area());

            frame.render_widget(ratatui::widgets::Clear, popup_area);

            let warning_box = Paragraph::new(target_name.clone())
                .block(
                    Block::default()
                        .title(format!(" Delete {} (Y/N)? ", target_name))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red))
                )
                .style(Style::default().fg(Color::White));

            frame.render_widget(warning_box, popup_area);
        } 
        
        if let AppMode::Renaming = self.mode {
            let popup_area = centered_rect(60, 3, frame.area());
            frame.render_widget(ratatui::widgets::Clear, popup_area);
            
            let input_box = Paragraph::new(self.rename_buffer.as_str()).block(
                Block::default().title("Rename").borders(Borders::ALL).border_style(Style::default().
                    fg(Color::Yellow)
                ));
            frame.render_widget(input_box, popup_area);

            #[allow(clippy::cast_possible_truncation)]
            frame.set_cursor_position((popup_area.x + 1 + self.rename_buffer.chars().count() as u16, popup_area.y + 1));
        }

        if let AppMode::Create = self.mode {
            let popup_area = centered_rect(50, 3, frame.area()); 
            frame.render_widget(ratatui::widgets::Clear, popup_area); 

            let input_area = Paragraph::new(self.create_buffer.clone())
                .block(
                    Block::default()
                    .title("Create File/Folder?")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                ).style(Style::default().fg(Color::White));

            frame.render_widget(input_area, popup_area);
            
            #[allow(clippy::cast_possible_truncation)]
            frame.set_cursor_position((popup_area.x + 1 + self.create_buffer.chars().count() as u16, popup_area.y + 1));
        }
        
        if let AppMode::Search = self.mode{
            let popup_area = top_right_rect(60,3,frame.area());
            frame.render_widget(Clear, popup_area);

            let search_box = Paragraph::new(self.search_buffer.clone()).block(
                Block::default().title("Search Box")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                )
                .style(Style::default().fg(Color::White));
        

        frame.render_widget(search_box, popup_area);

        #[allow(clippy::cast_possible_truncation)]
        frame.set_cursor_position((popup_area.x + 1 + self.create_buffer.chars().count() as u16, popup_area.y + 1));
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
                
                self.notification = None;

                match self.mode {
                    AppMode::Normal => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => break,
                        KeyCode::Up => self.cursor_up(),
                        KeyCode::Down => self.cursor_down(),
                        KeyCode::Enter => self.enter_selected(),
                        KeyCode::Backspace => self.go_up(),
                        KeyCode::Char('s') => self.cycle_sort(),
                        KeyCode::Char('d') => {
                            if self.get_selected_name().is_some() {
                                self.mode = AppMode::ConfirmDelete;
                            }
                        },
                        KeyCode::Char('/') => {
                            self.mode = AppMode::Search;
                            self.search_buffer.clear();
                        },
                        KeyCode::Char(']') => {
                            self.show_hidden = if self.show_hidden { false } else { true };
                            self.populate_files();
                        },
                        KeyCode::Char('r') => { 
                            if let Some(name) = self.get_selected_name() {
                                self.rename_buffer = name;
                                self.mode = AppMode::Renaming;
                            }
                        },
                        KeyCode::Char('c') => self.copy_item(),
                        KeyCode::Char('v') => self.paste_item(),
                        KeyCode::Char('n') => { // FIXED: Capital 'C' in Char
                            self.mode = AppMode::Create;
                            self.create_buffer.clear();
                        },
                        KeyCode::Char('h') => {
                            self.mode = AppMode::History;
                        },
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
                    AppMode::Renaming => match key.code {
                        KeyCode::Char(c) => self.rename_buffer.push(c),
                        KeyCode::Backspace => { self.rename_buffer.pop(); },
                        KeyCode::Enter => self.rename_item(),
                        KeyCode::Esc => {
                            self.rename_buffer.clear();
                            self.mode = AppMode::Normal;
                        },
                        _ => {} // FIXED: Added catch-all to satisfy the compiler
                    },
                    AppMode::Create => match key.code {
                        KeyCode::Esc => {
                            self.mode = AppMode::Normal;      
                        },
                        KeyCode::Char(c) => {
                            self.create_buffer.push(c); // FIXED: Added .push()
                        },
                        KeyCode::Backspace => {
                            self.create_buffer.pop();
                        },
                        KeyCode::Enter => {
                            self.create();                        },
                        _ => {}
                    },

                    AppMode::History => match key.code {
                        KeyCode::Char('h') | KeyCode::Esc => {
                            self.mode = AppMode::Normal;
                        }
                        _ => {}
                    },

                    AppMode::Search => match key.code {
                            KeyCode::Char(c) => {
                                self.search_buffer.push(c);
                                self.populate_files(); 
                            },
                            KeyCode::Backspace => {
                                self.search_buffer.pop();
                                self.populate_files();
                            },
                            KeyCode::Enter => {
                                self.mode = AppMode::Normal;
                            },
                            KeyCode::Esc => {
                                self.search_buffer.clear();
                                self.mode = AppMode::Normal;
                                self.populate_files();
                            },
                            _ => {}
                        },
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

pub fn right_panel_rect(fixed_x: u16, r: Rect) -> Rect {
    let horizontal_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),          // The main file list takes the rest
            Constraint::Length(fixed_x), // The exact width of the action log
        ])
        .split(r);

    horizontal_split[1]
}

pub fn top_right_rect(fixed_x: u16, fixed_y: u16, r: Rect) -> Rect {
    // 1. Slice vertically to grab the top portion
    let vertical_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(fixed_y), // The exact height at the top
            Constraint::Min(0),          // The rest of the screen below
        ])
        .split(r);

    // 2. Slice horizontally to grab the right portion of that top slice
    let horizontal_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),          // The rest of the screen to the left
            Constraint::Length(fixed_x), // The exact width at the right edge
        ])
        .split(vertical_split[0]);

    // Return the top-right chunk
    horizontal_split[1]
}

fn populate_search_results(current_path: &PathBuf, search_buffer: &str, show_hidden: bool, items: &mut Vec<DirItem>) {
    let mut dirs_to_visit = vec![current_path.clone()];
        let search_term = search_buffer.to_lowercase();

        while let Some(current_dir) = dirs_to_visit.pop() {
            if let Ok(entries) = fs::read_dir(&current_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let file_name = entry.file_name().to_string_lossy().into_owned();
                    
                    if !show_hidden && file_name.starts_with('.') {
                        continue;
                    }

                    if path.is_dir() {
                        dirs_to_visit.push(path.clone());
                    }

                    if file_name.to_lowercase().contains(&search_term) {
                        let display_name = path.strip_prefix(&current_path)
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or(file_name);

                        let is_dir = path.is_dir();
                        let (file_size, modified) = if let Ok(metadata) = entry.metadata() {
                            (metadata.len(), metadata.modified().ok())
                        } else {
                            (0, None)
                        };

                        items.push(DirItem { name: display_name, is_dir, file_size, modified });
                    }
                }
            }
        }
    }

fn is_text_file(path: &std::path::Path) -> bool {
    if let Ok(mut file) = std::fs::File::open(path) {
    let mut buffer = [0u8; 512];
    
    if let Ok(bytes_read) = file.read(&mut buffer) {
            if bytes_read == 0 {
                return true;
            }
            
            return !buffer.iter().take(bytes_read).any(|&byte| byte == 0);}
    }
    false
}

fn _is_image_file(path: &std::path::Path) -> bool {
    let ext = path.extension().unwrap_or_default().to_string_lossy().to_lowercase(); 
    return matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp")
}
// === MAIN ===
fn main() -> io::Result<()> {
    let mut app = App::new();
    app.populate_files();
    app.run()?;
    Ok(())
}
