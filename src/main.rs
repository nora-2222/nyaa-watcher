#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use slint::{Model, ModelRc, VecModel};
use std::collections::HashMap;
use std::thread;

mod nyaa;

// Config
fn config_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("nyaa-watcher.cfg")
}

fn load_config_path() -> String {
    std::fs::read_to_string(config_path())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn save_config_path(path: &str) {
    let _ = std::fs::write(config_path(), path);
}

fn default_download_path() -> String {
    let saved = load_config_path();
    if !saved.is_empty() {
        return saved;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("torrent")))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".\\torrent".to_string())
}

// Search
fn do_search(window_weak: slint::Weak<MainWindow>, query: String, page: i32) {
    if let Some(win) = window_weak.upgrade() {
        win.set_loading(true);
        win.set_status_text("Searching...".into());
        win.set_entries(ModelRc::new(VecModel::from(vec![])));
        win.set_all_checked(false);
    }

    let ww = window_weak.clone();
    thread::spawn(move || {
        let result = nyaa::search(&query, page);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(win) = ww.upgrade() else { return };
            win.set_loading(false);
            match result {
                Ok(data) => {
                    let count = data.entries.len();

                    // Collect (index > category_code) before consuming entries
                    let mut code_to_indices: HashMap<String, Vec<usize>> = HashMap::new();
                    for (idx, e) in data.entries.iter().enumerate() {
                        if !e.category_code.is_empty() && e.category_code != "0_0" {
                            code_to_indices
                                .entry(e.category_code.clone())
                                .or_default()
                                .push(idx);
                        }
                    }

                    let entries: Vec<TorrentEntry> = data
                        .entries
                        .into_iter()
                        .map(|e| TorrentEntry {
                            category: e.category.into(),
                            category_icon: Default::default(),
                            name: e.name.into(),
                            view_link: e.view_link.into(),
                            download_link: e.download_link.into(),
                            magnet_link: e.magnet_link.into(),
                            size: e.size.into(),
                            date: e.date.into(),
                            seeders: e.seeders.into(),
                            leechers: e.leechers.into(),
                            completed: e.completed.into(),
                            row_type: e.row_type,
                            checked: false,
                        })
                        .collect();

                    win.set_page_numbers(ModelRc::new(VecModel::from(data.page_list)));
                    win.set_entries(ModelRc::new(VecModel::from(entries)));
                    win.set_current_page(data.current_page);
                    win.set_total_pages(data.total_pages);
                    win.set_current_query(query.into());
                    win.set_status_text(
                        format!(
                            "{} results  —  page {} / {}",
                            count, data.current_page, data.total_pages
                        )
                        .into(),
                    );

                    // Fetch icons for each unique category code
                    for (code, indices) in code_to_indices {
                        let ww2 = ww.clone();
                        thread::spawn(move || {
                            if let Some(png_bytes) = nyaa::fetch_icon_bytes(&code) {
                                if let Ok(img) = image::load_from_memory(&png_bytes) {
                                    // Decode to raw RGBA bytes on background thread
                                    let rgba = img.to_rgba8();
                                    let (w, h) = (rgba.width(), rgba.height());
                                    let raw: Vec<u8> = rgba.into_raw();
                                    // Create slint::Image on main thread (not Send)
                                    let _ = slint::invoke_from_event_loop(move || {
                                        let Some(win) = ww2.upgrade() else { return };
                                        let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                                        buf.make_mut_bytes().copy_from_slice(&raw);
                                        let slint_img = slint::Image::from_rgba8(buf);
                                        let model = win.get_entries();
                                        if let Some(vm) = model.as_any().downcast_ref::<VecModel<TorrentEntry>>() {
                                            for idx in indices {
                                                if let Some(mut entry) = vm.row_data(idx) {
                                                    entry.category_icon = slint_img.clone();
                                                    vm.set_row_data(idx, entry);
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                        });
                    }
                }
                Err(e) => win.set_status_text(format!("Error: {e}").into()),
            }
        });
    });
}

// Helpers
fn set_checked(win: &MainWindow, index: usize, checked: bool) {
    let model = win.get_entries();
    if let Some(vec_model) = model.as_any().downcast_ref::<VecModel<TorrentEntry>>() {
        if let Some(mut entry) = vec_model.row_data(index) {
            entry.checked = checked;
            vec_model.set_row_data(index, entry);
        }
        let count = vec_model.row_count();
        let all = count > 0
            && (0..count).filter_map(|i| vec_model.row_data(i)).all(|e| e.checked);
        win.set_all_checked(all);
    }
}

fn set_all_checked(win: &MainWindow, checked: bool) {
    win.set_all_checked(checked);
    let model = win.get_entries();
    if let Some(vec_model) = model.as_any().downcast_ref::<VecModel<TorrentEntry>>() {
        for i in 0..vec_model.row_count() {
            if let Some(mut e) = vec_model.row_data(i) {
                e.checked = checked;
                vec_model.set_row_data(i, e);
            }
        }
    }
}

#[allow(dead_code)]
fn checked_download_entries(win: &MainWindow) -> Vec<(String, String)> {
    let model = win.get_entries();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|e| e.checked)
        .map(|e| (e.name.to_string(), e.download_link.to_string()))
        .collect()
}

// Main
fn main() -> Result<(), slint::PlatformError> {
    #[cfg(windows)]
    unsafe {
        use std::os::raw::{c_int, c_uint};
        extern "system" {
            fn SetConsoleOutputCP(id: c_uint) -> i32;
            fn GetSystemMetrics(nIndex: c_int) -> c_int;
            fn GetDpiForSystem() -> c_uint;
        }
        SetConsoleOutputCP(65001);

        // Set SLINT_SCALE_FACTOR before window creation so all px values scale correctly.
        // Formula: keep OS DPI scale, but add extra if logical screen is wider than FHD.
        // - FHD at 100%: scale=1.0  (baseline)
        // - 4K  at 100%: logical=3840 → scale=2.0  (everything 2× bigger)
        // - 4K  at 200%: logical=1920 → scale=2.0  (OS DPI already at 2×, no change)
        // - 2K  at 100%: logical=2560 → scale=1.33
        let screen_phys_w = GetSystemMetrics(0) as f64;
        let os_scale = GetDpiForSystem() as f64 / 96.0;
        let logical_w = screen_phys_w / os_scale;
        let extra = (logical_w / 1920.0).max(1.0); // never shrink below OS DPI
        let effective = (os_scale * extra).clamp(0.75, 4.0);
        std::env::set_var("SLINT_SCALE_FACTOR", format!("{:.3}", effective));
    }

    let window = MainWindow::new()?;

    // Center window on screen
    #[cfg(windows)]
    {
        use std::os::raw::c_int;
        extern "system" { fn GetSystemMetrics(nIndex: c_int) -> c_int; }
        let screen_phys_w = unsafe { GetSystemMetrics(0) } as i32;
        let screen_phys_h = unsafe { GetSystemMetrics(1) } as i32;
        let scale = window.window().scale_factor() as f64;
        let win_phys_w = (1280.0 * scale) as i32;
        let win_phys_h = (800.0 * scale) as i32;
        let x = ((screen_phys_w - win_phys_w) / 2).max(0);
        let y = ((screen_phys_h - win_phys_h) / 2).max(0);
        window.window().set_position(slint::PhysicalPosition::new(x, y));
    }

    // Initial download path from config or default
    window.set_download_path(default_download_path().into());

    // Initial load
    do_search(window.as_weak(), String::new(), 1);

    // Search
    window.on_search_requested({
        let w = window.as_weak();
        move |q| do_search(w.clone(), q.to_string(), 1)
    });

    // Pagination
    window.on_page_changed({
        let w = window.as_weak();
        move |page| {
            if let Some(win) = w.upgrade() {
                let q = win.get_current_query().to_string();
                do_search(w.clone(), q, page);
            }
        }
    });

    // Checkbox per row
    window.on_toggle_checked({
        let w = window.as_weak();
        move |index, checked| {
            if let Some(win) = w.upgrade() {
                set_checked(&win, index as usize, checked);
            }
        }
    });

    // Header checkbox
    window.on_toggle_all_checked({
        let w = window.as_weak();
        move |checked| {
            if let Some(win) = w.upgrade() {
                set_all_checked(&win, checked);
            }
        }
    });

    // Name click > fetch detail and show popup
    window.on_name_clicked({
        let w = window.as_weak();
        move |view_link| {
            if let Some(win) = w.upgrade() {
                win.set_show_detail(true);
                win.set_detail_loading(true);
            }
            let url = view_link.to_string();
            let ww = w.clone();
            thread::spawn(move || {
                let result = nyaa::get_detail(&url);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(win) = ww.upgrade() else { return };
                    win.set_detail_loading(false);
                    match result {
                        Ok(d) => win.set_detail(TorrentDetail {
                            title: d.title.into(),
                            category: d.category.into(),
                            date: d.date.into(),
                            submitter: d.submitter.into(),
                            seeders: d.seeders.into(),
                            leechers: d.leechers.into(),
                            file_size: d.file_size.into(),
                            completed: d.completed.into(),
                            info_hash: d.info_hash.into(),
                            download_link: d.download_link.into(),
                            magnet_link: d.magnet_link.into(),
                        }),
                        Err(e) => eprintln!("Detail error: {e}"),
                    }
                });
            });
        }
    });

    // Close detail popup
    window.on_close_detail({
        let w = window.as_weak();
        move || {
            if let Some(win) = w.upgrade() {
                win.set_show_detail(false);
            }
        }
    });

    // Download panel open/close
    window.on_show_download_panel({
        let w = window.as_weak();
        move || {
            if let Some(win) = w.upgrade() {
                win.set_show_download(true);
            }
        }
    });

    window.on_close_download_panel({
        let w = window.as_weak();
        move || {
            if let Some(win) = w.upgrade() {
                win.set_show_download(false);
            }
        }
    });

    // Grid Downliad status button
    window.on_download_single({
        let w = window.as_weak();
        move |index| {
            let Some(win) = w.upgrade() else { return };
            let model = win.get_entries();
            if let Some(entry) = model.row_data(index as usize) {
                let new_item = DownloadItem {
                    name: entry.name.clone(),
                    download_link: entry.download_link.clone(),
                    completed: false,
                };
                let queue = win.get_download_queue();
                let mut existing: Vec<DownloadItem> = (0..queue.row_count())
                    .filter_map(|i| queue.row_data(i))
                    .collect();
                if !existing.iter().any(|e| e.name == new_item.name) {
                    existing.push(new_item);
                }
                win.set_download_queue(ModelRc::new(VecModel::from(existing)));
            }
            win.set_show_download(true);
        }
    });

    // "Download checked items" button > add to independent queue
    window.on_add_to_queue({
        let w = window.as_weak();
        move || {
            let Some(win) = w.upgrade() else { return };
            let new_items: Vec<DownloadItem> = {
                let model = win.get_entries();
                (0..model.row_count())
                    .filter_map(|i| model.row_data(i))
                    .filter(|e| e.checked)
                    .map(|e| DownloadItem { name: e.name.clone(), download_link: e.download_link.clone(), completed: false })
                    .collect()
            };
            // Merge into existing queue, skip duplicates by name
            let queue = win.get_download_queue();
            let mut existing: Vec<DownloadItem> = (0..queue.row_count())
                .filter_map(|i| queue.row_data(i))
                .collect();
            for item in new_items {
                if !existing.iter().any(|e| e.name == item.name) {
                    existing.push(item);
                }
            }
            win.set_download_queue(ModelRc::new(VecModel::from(existing)));
            win.set_show_download(true);
        }
    });

    // Remove single item from download queue
    window.on_remove_from_queue({
        let w = window.as_weak();
        move |index| {
            let Some(win) = w.upgrade() else { return };
            let queue = win.get_download_queue();
            let mut items: Vec<DownloadItem> = (0..queue.row_count())
                .filter_map(|i| queue.row_data(i))
                .collect();
            let idx = index as usize;
            if idx < items.len() {
                items.remove(idx);
                win.set_download_queue(ModelRc::new(VecModel::from(items)));
            }
        }
    });

    // Remove completed items from download queue
    window.on_clear_completed({
        let w = window.as_weak();
        move || {
            let Some(win) = w.upgrade() else { return };
            let queue = win.get_download_queue();
            let remaining: Vec<DownloadItem> = (0..queue.row_count())
                .filter_map(|i| queue.row_data(i))
                .filter(|item| !item.completed)
                .collect();
            win.set_download_queue(ModelRc::new(VecModel::from(remaining)));
        }
    });

    // Clear all items from download queue
    window.on_clear_queue({
        let w = window.as_weak();
        move || {
            let Some(win) = w.upgrade() else { return };
            win.set_download_queue(ModelRc::new(VecModel::from(vec![])));
        }
    });

    // Detail popup "Download Torrent" > add to queue and open download panel
    window.on_download_detail({
        let w = window.as_weak();
        move || {
            let Some(win) = w.upgrade() else { return };
            let detail = win.get_detail();
            let new_item = DownloadItem {
                name: detail.title.clone(),
                download_link: detail.download_link.clone(),
                completed: false,
            };
            let queue = win.get_download_queue();
            let mut existing: Vec<DownloadItem> = (0..queue.row_count())
                .filter_map(|i| queue.row_data(i))
                .collect();
            if !existing.iter().any(|e| e.name == new_item.name) {
                existing.push(new_item);
            }
            win.set_download_queue(ModelRc::new(VecModel::from(existing)));
            win.set_show_detail(false);
            win.set_show_download(true);
        }
    });

    // Browse for save folder
    window.on_browse_path({
        let w = window.as_weak();
        move || {
            let ww = w.clone();
            thread::spawn(move || {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    let path = folder.to_string_lossy().into_owned();
                    save_config_path(&path);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = ww.upgrade() {
                            win.set_download_path(path.into());
                        }
                    });
                }
            });
        }
    });

    // Batch download .torrent files
    window.on_start_download({
        let w = window.as_weak();
        move |save_path| {
            let save_path = save_path.to_string();
            let entries: Vec<(String, String)> = w
                .upgrade()
                .map(|win| {
                    let queue = win.get_download_queue();
                    (0..queue.row_count())
                        .filter_map(|i| queue.row_data(i))
                        .map(|item| (item.name.to_string(), item.download_link.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            if entries.is_empty() {
                if let Some(win) = w.upgrade() {
                    win.set_status_text("No items checked.".into());
                    win.set_download_status("No items selected.".into());
                }
                return;
            }

            if let Some(win) = w.upgrade() {
                win.set_status_text(
                    format!("Downloading {} torrents...", entries.len()).into(),
                );
                win.set_download_status("Starting...".into());
                win.set_is_downloading(true);
            }

            save_config_path(&save_path);

            let ww = w.clone();
            let sp = save_path.clone();
            thread::spawn(move || {
                let ww2 = ww.clone();
                let result = nyaa::download_torrents(&entries, &sp, move |idx, total, name, is_done| {
                    let msg = if is_done {
                        format!("[{}/{}] done: {}", idx + 1, total, name)
                    } else {
                        format!("[{}/{}] {}", idx + 1, total, name)
                    };
                    let ww3 = ww2.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(win) = ww3.upgrade() else { return };
                        win.set_download_status(msg.into());
                        if is_done {
                            // Mark this item as completed in the queue
                            let queue = win.get_download_queue();
                            if let Some(vm) = queue.as_any().downcast_ref::<VecModel<DownloadItem>>() {
                                if let Some(mut item) = vm.row_data(idx) {
                                    item.completed = true;
                                    vm.set_row_data(idx, item);
                                }
                            }
                        }
                    });
                });
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = ww.upgrade() {
                        win.set_is_downloading(false);
                        match result {
                            Ok(n) => {
                                win.set_download_status(
                                    format!("Done: {} files saved", n).into(),
                                );
                                win.set_status_text(
                                    format!("{} torrents saved to {}", n, sp).into(),
                                );
                            }
                            Err(e) => {
                                win.set_download_status(format!("Error: {}", e).into());
                                win.set_status_text(format!("Download error: {}", e).into());
                            }
                        }
                    }
                });
            });
        }
    });

    // Open .torrent in browser
    window.on_open_torrent(|url| {
        let _ = open::that(url.as_str());
    });

    // Open magnet link
    window.on_open_magnet(|url| {
        let _ = open::that(url.as_str());
    });

    window.run()
}
