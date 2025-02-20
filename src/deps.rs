use elf::{
    dynamic::Dyn, endian::AnyEndian, parse::ParsingTable, string_table::StringTable, abi
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, List, ListDirection, ListState, Paragraph, StatefulWidget, Widget},
};
use std::collections::HashMap;
use std::process::Command;
use std::env::consts::{ARCH, OS};

pub struct DependenciesPage<'a> {
    pub rpath: Option<String>,
    pub needed: Vec<DependencyEntry<'a>>,
    pub list: List<'a>,
    pub state: ListState,
}

pub struct DependencyEntry<'a> {
    pub name: &'a str,
    pub is_critical: bool,
    pub search_path: String,
    pub actual_path: String,
}

impl<'a> DependenciesPage<'a> {
    fn get_actual_library_paths(interpreter: Option<&str>, elf_path: &str) -> HashMap<String, String> {
        let mut library_paths = HashMap::new();
        
        // 只在 Linux 系统上执行
        if OS != "linux" {
            return library_paths;
        }

        // 检查架构是否匹配
        let current_arch = match ARCH {
            "x86_64" => true,
            _ => false,
        };

        if !current_arch {
            return library_paths;
        }
        
        // 使用实际的 ELF 文件路径
        if let Some(interpreter) = interpreter {
            if let Ok(output) = Command::new(interpreter)
                .arg("--list")
                .arg(elf_path)  // 使用实际的 ELF 文件路径
                .output()
            {
                let output_str = String::from_utf8_lossy(&output.stdout);
                log::info!("{}", output_str);
                
                for line in output_str.lines() {
                    if line.contains("=>") {
                        let parts: Vec<&str> = line.split("=>").collect();
                        if parts.len() >= 2 {
                            let lib_name = parts[0].trim().to_string();
                            let lib_path = parts[1]
                                .split('(')
                                .next()
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            
                            if !lib_path.is_empty() {
                                library_paths.insert(lib_name, lib_path);
                            }
                        }
                    }
                }
            }
        }
        
        library_paths
    }

    pub fn new(
        dynamic: Option<ParsingTable<'a, AnyEndian, Dyn>>,
        dynstr: Option<StringTable<'a>>,
        interpreter: Option<&str>,
        elf_path: &str,  // 新增参数
    ) -> DependenciesPage<'a> {
        let mut rpath = None;
        let mut needed = Vec::new();
        
        // 传入 ELF 文件路径
        let actual_paths = Self::get_actual_library_paths(interpreter, elf_path);
        let can_show_actual_paths = !actual_paths.is_empty();

        // Get dynamic section
        if let Some(dynamic) = dynamic {
            // Extract RPATH
            if let Some(rpath_entry) = dynamic.iter().find(|d| d.d_tag == abi::DT_RPATH) {
                if let Some(dynstr) = &dynstr {
                    if let Ok(path) = dynstr.get(rpath_entry.d_val() as usize) {
                        rpath = Some(path.to_string());
                    }
                }
            }

            // Extract needed libraries
            if let Some(dynstr) = &dynstr {
                for entry in dynamic.iter() {
                    if entry.d_tag == abi::DT_NEEDED {
                        if let Ok(name) = dynstr.get(entry.d_val() as usize) {
                            let is_critical = Self::is_critical_library(name);
                            let actual_path = if can_show_actual_paths {
                                actual_paths
                                    .get(name)
                                    .cloned()
                                    .unwrap_or_else(|| "Not found".to_string())
                            } else {
                                "Not available on current platform".to_string()
                            };
                            
                            needed.push(DependencyEntry {
                                name,
                                is_critical,
                                search_path: Self::get_search_path(name, rpath.as_deref()),
                                actual_path,
                            });
                        }
                    }
                }
            }
        }

        let list_items = needed.iter().map(|entry| {
            if entry.is_critical {
                format!("* {}", entry.name)
            } else {
                entry.name.to_string()
            }
        }).collect::<Vec<_>>();

        let list = List::new(list_items)
            .block(Block::bordered().title("Dependencies"))
            .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
            .highlight_symbol(">> ")
            .repeat_highlight_symbol(true)
            .direction(ListDirection::TopToBottom);

        DependenciesPage {
            rpath,
            needed,
            list,
            state: ListState::default(),
        }
    }

    fn is_critical_library(name: &str) -> bool {
        let critical_libs = [
            "libc.so",
            "libstdc++.so",
            "libgcc_s.so",
            "ld-linux",
        ];
        critical_libs.iter().any(|lib| name.starts_with(lib))
    }

    fn get_search_path(name: &str, rpath: Option<&str>) -> String {
        let mut paths = Vec::new();
        
        // 1. RPATH/RUNPATH
        if let Some(rpath) = rpath {
            paths.push(rpath.to_string());
        }
        
        // 2. LD_LIBRARY_PATH (environment variable)
        if let Ok(ld_path) = std::env::var("LD_LIBRARY_PATH") {
            paths.push(ld_path);
        }
        
        // 3. Default system paths
        paths.extend_from_slice(&[
            "/lib".to_string(),
            "/usr/lib".to_string(),
            "/lib64".to_string(),
            "/usr/lib64".to_string(),
        ]);

        paths.join(":")
    }
}

impl Widget for &mut DependenciesPage<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Min(40), Constraint::Percentage(100)])
            .split(area);

        StatefulWidget::render(&self.list, layout[0], buf, &mut self.state);

        let details = if let Some(selected) = self.state.selected() {
            let entry = &self.needed[selected];
            let mut lines = vec![
                Line::from(vec![
                    Span::raw("Library: "),
                    Span::styled(entry.name, Style::default().add_modifier(Modifier::BOLD)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::raw("Type: "),
                    Span::styled(
                        if entry.is_critical { "Critical System Library" } else { "Regular Library" },
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from("Search Paths:"),
            ];

            // 将搜索路径按 : 分割并添加到行中
            for path in entry.search_path.split(':') {
                if !path.is_empty() {
                    lines.push(Line::from(format!("  {}", path)));
                }
            }

            // 只在 Linux 且架构匹配时显示实际路径
            if OS == "linux" && ARCH == "x86_64" {
                lines.extend_from_slice(&[
                    Line::from(""),
                    Line::from("Actual Path:"),
                    Line::from(Span::styled(
                        &entry.actual_path,
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                ]);
            }

            lines
        } else {
            let mut lines = vec![
                Line::from("Select a library to view details"),
                Line::from(""),
                Line::from("* Critical system libraries are marked with an asterisk"),
            ];
            lines
        };

        Paragraph::new(details)
            .block(Block::bordered().title("Library Details"))
            .render(layout[1], buf);
    }
}
