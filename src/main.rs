use chrono::{DateTime, Local};
use eframe::egui;
use human_bytes::human_bytes;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct FileExplorer {
    current_path: PathBuf,
    entries: Vec<PathBuf>,
    error_message: Option<String>,
    selected_entry: Option<usize>,
    path_to_navigate: Option<PathBuf>,
    needs_repaint: bool,
}

impl Default for FileExplorer {
    fn default() -> Self {
        let current_path = std::env::current_dir().unwrap_or_default();
        let mut app = Self {
            current_path,
            entries: Vec::new(),
            error_message: None,
            selected_entry: None,
            path_to_navigate: None,
            needs_repaint: false,
        };
        app.refresh_entries();
        app
    }
}

fn open_file_with_default_app(path: &Path) -> Result<(), String> {
    let command = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(&["/C", "start", "", path.to_str().unwrap_or_default()])
            .spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open")
            .arg(path.to_str().unwrap_or_default())
            .spawn()
    } else {
        // Assume Linux/Unix
        Command::new("xdg-open")
            .arg(path.to_str().unwrap_or_default())
            .spawn()
    };

    match command {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to open file: {}", e)),
    }
}

impl FileExplorer {
    fn refresh_entries(&mut self) {
        self.entries.clear();
        match fs::read_dir(&self.current_path) {
            Ok(entries) => {
                self.error_message = None;

                // Add parent directory (..) unless we're at the root
                if self.current_path.parent().is_some() {
                    self.entries.push(self.current_path.join(".."));
                }

                // Add all entries in the current directory
                for entry in entries {
                    if let Ok(entry) = entry {
                        self.entries.push(entry.path());
                    }
                }

                // Sort entries: directories first, then files
                self.entries.sort_by(|a, b| {
                    let a_is_dir = a.is_dir();
                    let b_is_dir = b.is_dir();

                    if a_is_dir && !b_is_dir {
                        std::cmp::Ordering::Less
                    } else if !a_is_dir && b_is_dir {
                        std::cmp::Ordering::Greater
                    } else {
                        a.file_name().cmp(&b.file_name())
                    }
                });
            }
            Err(e) => {
                self.error_message = Some(format!("Error reading directory: {}", e));
            }
        }
    }

    fn navigate_to(&mut self, path: PathBuf) {
        // Handle ".." (parent directory) specially
        if path.ends_with("..") {
            if let Some(parent) = self.current_path.parent() {
                self.current_path = parent.to_path_buf();
                self.refresh_entries();
                self.needs_repaint = true;
                eprintln!("Navigated to parent: {:?}", self.current_path);
            }
            return;
        }

        // Add explicit debug output to help diagnose issues
        eprintln!(
            "Attempting to navigate to: {:?}, is_dir: {}",
            path,
            path.is_dir()
        );

        if path.is_dir() {
            self.current_path = path;
            self.refresh_entries();
            self.needs_repaint = true;
            eprintln!("Successfully navigated to: {:?}", self.current_path);
        } else {
            eprintln!("Not navigating: path is not a directory");
        }
    }

    fn get_file_info(&self, path: &Path) -> (String, String) {
        let size;
        let modified;

        if let Ok(metadata) = fs::metadata(path) {
            if path.is_dir() {
                size = "<DIR>".to_string();
            } else {
                size = human_bytes(metadata.len() as f64);
            }

            if let Ok(time) = metadata.modified() {
                let datetime: DateTime<Local> = time.into();
                modified = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
            } else {
                modified = "".to_string();
            }
        } else {
            size = "".to_string();
            modified = "".to_string();
        }

        (size, modified)
    }
}

impl eframe::App for FileExplorer {
    // Handle cleanup on exit to prevent Wayland warnings
    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        // Force drop of any resources that might be held
        self.entries.clear();
        self.error_message = None;
        self.selected_entry = None;
        self.path_to_navigate = None;
        // Explicitly drop any remaining Wayland resources
        let _ = gl;
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle navigation from previous frame (to avoid borrow issues)
        if let Some(path) = self.path_to_navigate.take() {
            self.navigate_to(path);
        }

        // Check if we need to repaint after navigation
        if self.needs_repaint {
            self.needs_repaint = false;
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("File Explorer");

            // Current path display
            ui.horizontal(|ui| {
                ui.label("Current path:");
                ui.label(self.current_path.to_string_lossy().to_string());
            });

            // Error message (if any)
            if let Some(error) = &self.error_message {
                ui.colored_label(egui::Color32::RED, error);
            }

            // File/directory list
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Store paths that need navigation to avoid borrow issues
                let mut clicked_path = None;

                for (idx, entry) in self.entries.iter().enumerate() {
                    let is_parent_dir = entry.ends_with("..");
                    let is_dir = is_parent_dir || entry.is_dir();

                    let display_name = if is_parent_dir {
                        "..".to_string()
                    } else {
                        entry
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string()
                    };

                    let (size, modified) = self.get_file_info(entry);

                    ui.horizontal(|ui| {
                        // For directories, make it more obvious it's clickable
                        let label_text = if is_dir {
                            format!("📁 {}", display_name)
                        } else {
                            display_name
                        };

                        // Use a button for directories and selectable for files
                        if is_dir {
                            if ui.button(&label_text).clicked() {
                                eprintln!("Directory button clicked: {:?}", entry);
                                clicked_path = Some(entry.clone());
                            }
                        } else {
                            let resp = ui.selectable_value(&mut self.selected_entry, Some(idx), &label_text);
                            if resp.double_clicked() {
                                if let Err(e) = open_file_with_default_app(&entry.clone()) {
                                    self.error_message = Some(e.clone());
                                }
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(modified);
                            ui.label(size);
                        });
                    });
                }

                // Handle navigation outside the loop to avoid borrow checker issues
                if let Some(path) = clicked_path {
                    self.path_to_navigate = Some(path);
                    ctx.request_repaint();
                }
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(800.0, 600.0)),
        ..Default::default()
    };
    eframe::run_native(
        "File Explorer",
        native_options,
        Box::new(|_cc| Box::new(FileExplorer::default())),
    )
}
