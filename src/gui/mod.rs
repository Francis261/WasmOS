use crate::host::{HostBridge, HostKind};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub type WindowId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowDescriptor {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: WindowId,
    pub owner_task_id: u64,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DrawCommand {
    Clear {
        rgba: [u8; 4],
    },
    Pixel {
        x: u32,
        y: u32,
        rgba: [u8; 4],
    },
    Text {
        x: u32,
        y: u32,
        text: String,
        rgba: [u8; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuiEvent {
    KeyDown { key_code: u32 },
    KeyUp { key_code: u32 },
    MouseMove { x: i32, y: i32 },
    MouseClick { x: i32, y: i32, button: u8 },
    CloseRequested,
}

trait TextRenderer: Send + Sync {
    fn draw_text(
        &self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        x: u32,
        y: u32,
        text: &str,
        rgba: [u8; 4],
    );
}

#[derive(Default)]
struct BlockTextRenderer;

impl TextRenderer for BlockTextRenderer {
    fn draw_text(
        &self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        x: u32,
        y: u32,
        text: &str,
        rgba: [u8; 4],
    ) {
        let glyph_w = 6;
        let glyph_h = 8;
        for (i, _ch) in text.chars().enumerate() {
            let base_x = x + (i as u32 * glyph_w);
            for dy in 0..glyph_h {
                for dx in 0..(glyph_w - 1) {
                    let px = base_x + dx;
                    let py = y + dy;
                    if px >= width || py >= height {
                        continue;
                    }
                    write_pixel(frame, width, px, py, rgba);
                }
            }
        }
    }
}

trait GuiBackend: Send {
    fn create_window(&mut self, window_id: WindowId, descriptor: &WindowDescriptor) -> Result<()>;
    fn draw(&mut self, window_id: WindowId, commands: &[DrawCommand]) -> Result<()>;
    fn poll_events(&mut self, window_id: WindowId) -> Result<Vec<GuiEvent>>;
    fn list_windows(&self) -> Vec<WindowId>;
}

#[derive(Default)]
struct InMemoryBackend {
    windows: BTreeMap<WindowId, MemoryWindow>,
    text_renderer: BlockTextRenderer,
}

struct MemoryWindow {
    descriptor: WindowDescriptor,
    frame: Vec<u8>,
    events: VecDeque<GuiEvent>,
}

impl GuiBackend for InMemoryBackend {
    fn create_window(&mut self, window_id: WindowId, descriptor: &WindowDescriptor) -> Result<()> {
        let frame = vec![0; (descriptor.width * descriptor.height * 4) as usize];
        self.windows.insert(
            window_id,
            MemoryWindow {
                descriptor: descriptor.clone(),
                frame,
                events: VecDeque::new(),
            },
        );
        Ok(())
    }

    fn draw(&mut self, window_id: WindowId, commands: &[DrawCommand]) -> Result<()> {
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or_else(|| anyhow!("window {window_id} not found"))?;
        apply_draw_commands(
            &self.text_renderer,
            &mut window.frame,
            window.descriptor.width,
            window.descriptor.height,
            commands,
        );
        Ok(())
    }

    fn poll_events(&mut self, window_id: WindowId) -> Result<Vec<GuiEvent>> {
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or_else(|| anyhow!("window {window_id} not found"))?;
        Ok(window.events.drain(..).collect())
    }

    fn list_windows(&self) -> Vec<WindowId> {
        self.windows.keys().copied().collect()
    }
}

#[cfg(feature = "desktop-gui")]
mod desktop {
    use super::*;
    use minifb::{KeyRepeat, MouseButton, Window, WindowOptions};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::thread;
    use std::time::Duration;

    pub struct DesktopBackend {
        tx: Sender<GuiCommand>,
        thread: Option<thread::JoinHandle<()>>,
    }

    enum GuiCommand {
        CreateWindow {
            window_id: WindowId,
            descriptor: WindowDescriptor,
            response: Sender<Result<(), String>>,
        },
        Draw {
            window_id: WindowId,
            commands: Vec<DrawCommand>,
            response: Sender<Result<(), String>>,
        },
        PollEvents {
            window_id: WindowId,
            response: Sender<Result<Vec<GuiEvent>, String>>,
        },
        ListWindows {
            response: Sender<Vec<WindowId>>,
        },
        Shutdown,
    }

    struct DesktopWindow {
        window: Window,
        descriptor: WindowDescriptor,
        rgba_frame: Vec<u8>,
        argb_frame: Vec<u32>,
        events: VecDeque<GuiEvent>,
        dirty: bool,
    }

    impl DesktopBackend {
        pub fn new() -> Self {
            let (tx, rx) = mpsc::channel::<GuiCommand>();
            let thread = thread::spawn(move || worker_loop(rx));
            Self {
                tx,
                thread: Some(thread),
            }
        }

        fn request<R>(
            &self,
            build: impl FnOnce(Sender<R>) -> GuiCommand,
        ) -> Result<R, anyhow::Error> {
            let (response_tx, response_rx) = mpsc::channel();
            self.tx
                .send(build(response_tx))
                .map_err(|_| anyhow!("desktop GUI thread unavailable"))?;
            response_rx
                .recv()
                .map_err(|_| anyhow!("desktop GUI response channel closed"))
        }
    }

    impl Drop for DesktopBackend {
        fn drop(&mut self) {
            let _ = self.tx.send(GuiCommand::Shutdown);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    impl GuiBackend for DesktopBackend {
        fn create_window(
            &mut self,
            window_id: WindowId,
            descriptor: &WindowDescriptor,
        ) -> Result<()> {
            let descriptor = descriptor.clone();
            let result = self.request(|response| GuiCommand::CreateWindow {
                window_id,
                descriptor,
                response,
            })?;
            result.map_err(anyhow::Error::msg)
        }

        fn draw(&mut self, window_id: WindowId, commands: &[DrawCommand]) -> Result<()> {
            let commands = commands.to_vec();
            let result = self.request(|response| GuiCommand::Draw {
                window_id,
                commands,
                response,
            })?;
            result.map_err(anyhow::Error::msg)
        }

        fn poll_events(&mut self, window_id: WindowId) -> Result<Vec<GuiEvent>> {
            let result = self.request(|response| GuiCommand::PollEvents {
                window_id,
                response,
            })?;
            result.map_err(anyhow::Error::msg)
        }

        fn list_windows(&self) -> Vec<WindowId> {
            self.request(|response| GuiCommand::ListWindows { response })
                .unwrap_or_default()
        }
    }

    fn worker_loop(rx: Receiver<GuiCommand>) {
        let mut windows = BTreeMap::<WindowId, DesktopWindow>::new();
        let text_renderer = BlockTextRenderer;

        loop {
            match rx.recv_timeout(Duration::from_millis(16)) {
                Ok(GuiCommand::CreateWindow {
                    window_id,
                    descriptor,
                    response,
                }) => {
                    let result = create_window(&mut windows, window_id, descriptor);
                    let _ = response.send(result);
                }
                Ok(GuiCommand::Draw {
                    window_id,
                    commands,
                    response,
                }) => {
                    let result = draw_window(&mut windows, &text_renderer, window_id, &commands);
                    let _ = response.send(result);
                }
                Ok(GuiCommand::PollEvents {
                    window_id,
                    response,
                }) => {
                    let result = poll_window_events(&mut windows, window_id);
                    let _ = response.send(result);
                }
                Ok(GuiCommand::ListWindows { response }) => {
                    let list = windows.keys().copied().collect::<Vec<_>>();
                    let _ = response.send(list);
                }
                Ok(GuiCommand::Shutdown) => break,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            for window in windows.values_mut() {
                collect_events(window);
                let _ = refresh(window);
            }
        }
    }

    fn create_window(
        windows: &mut BTreeMap<WindowId, DesktopWindow>,
        window_id: WindowId,
        descriptor: WindowDescriptor,
    ) -> Result<(), String> {
        let window = Window::new(
            &descriptor.title,
            descriptor.width as usize,
            descriptor.height as usize,
            WindowOptions::default(),
        )
        .map_err(|error| error.to_string())?;
        windows.insert(
            window_id,
            DesktopWindow {
                window,
                descriptor: descriptor.clone(),
                rgba_frame: vec![0; (descriptor.width * descriptor.height * 4) as usize],
                argb_frame: vec![0; (descriptor.width * descriptor.height) as usize],
                events: VecDeque::new(),
                dirty: true,
            },
        );
        Ok(())
    }

    fn draw_window(
        windows: &mut BTreeMap<WindowId, DesktopWindow>,
        text_renderer: &BlockTextRenderer,
        window_id: WindowId,
        commands: &[DrawCommand],
    ) -> Result<(), String> {
        let Some(window) = windows.get_mut(&window_id) else {
            return Err(format!("window {window_id} not found"));
        };
        apply_draw_commands(
            text_renderer,
            &mut window.rgba_frame,
            window.descriptor.width,
            window.descriptor.height,
            commands,
        );
        window.dirty = true;
        refresh(window)
    }

    fn poll_window_events(
        windows: &mut BTreeMap<WindowId, DesktopWindow>,
        window_id: WindowId,
    ) -> Result<Vec<GuiEvent>, String> {
        let Some(window) = windows.get_mut(&window_id) else {
            return Err(format!("window {window_id} not found"));
        };
        collect_events(window);
        Ok(window.events.drain(..).collect())
    }

    fn collect_events(window: &mut DesktopWindow) {
        for key in window.window.get_keys_pressed(KeyRepeat::No) {
            window.events.push_back(GuiEvent::KeyDown {
                key_code: key as u32,
            });
        }
        if let Some((x, y)) = window.window.get_mouse_pos(minifb::MouseMode::Discard) {
            window.events.push_back(GuiEvent::MouseMove {
                x: x as i32,
                y: y as i32,
            });
        }
        if window.window.get_mouse_down(MouseButton::Left)
            && let Some((x, y)) = window.window.get_mouse_pos(minifb::MouseMode::Discard)
        {
            window.events.push_back(GuiEvent::MouseClick {
                x: x as i32,
                y: y as i32,
                button: 0,
            });
        }
        if !window.window.is_open() {
            window.events.push_back(GuiEvent::CloseRequested);
        }
    }

    fn refresh(window: &mut DesktopWindow) -> Result<(), String> {
        if !window.dirty {
            return Ok(());
        }
        for (index, pixel) in window.rgba_frame.chunks_exact(4).enumerate() {
            window.argb_frame[index] = ((pixel[3] as u32) << 24)
                | ((pixel[0] as u32) << 16)
                | ((pixel[1] as u32) << 8)
                | pixel[2] as u32;
        }
        window
            .window
            .update_with_buffer(
                &window.argb_frame,
                window.descriptor.width as usize,
                window.descriptor.height as usize,
            )
            .map_err(|error| error.to_string())?;
        window.dirty = false;
        Ok(())
    }
}

pub struct GuiSubsystem {
    host: Arc<HostBridge>,
    windows: RwLock<BTreeMap<WindowId, WindowInfo>>,
    backend: Mutex<Box<dyn GuiBackend>>,
    next_window_id: Mutex<u64>,
}

impl std::fmt::Debug for GuiSubsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuiSubsystem").finish_non_exhaustive()
    }
}

impl GuiSubsystem {
    pub fn new(host: Arc<HostBridge>) -> Self {
        #[cfg(feature = "desktop-gui")]
        let backend: Box<dyn GuiBackend> = if matches!(host.kind(), HostKind::Desktop) {
            Box::new(desktop::DesktopBackend::new())
        } else {
            Box::new(InMemoryBackend::default())
        };

        #[cfg(not(feature = "desktop-gui"))]
        let backend: Box<dyn GuiBackend> = Box::new(InMemoryBackend::default());

        Self {
            host,
            windows: RwLock::new(BTreeMap::new()),
            backend: Mutex::new(backend),
            next_window_id: Mutex::new(1),
        }
    }

    pub async fn create_window(&self, task_id: u64, descriptor: WindowDescriptor) -> WindowId {
        let window_id = self.allocate_window_id(task_id).await;
        let info = WindowInfo {
            id: window_id,
            owner_task_id: task_id,
            title: descriptor.title.clone(),
            width: descriptor.width,
            height: descriptor.height,
        };
        self.windows.write().await.insert(window_id, info);
        let _ = self
            .backend
            .lock()
            .await
            .create_window(window_id, &descriptor);
        window_id
    }

    pub async fn draw(&self, _task_id: u64, window_id: WindowId, commands: Vec<DrawCommand>) {
        let _ = self.backend.lock().await.draw(window_id, &commands);
    }

    pub async fn poll_events(&self, _task_id: u64, window_id: WindowId) -> Vec<GuiEvent> {
        self.backend
            .lock()
            .await
            .poll_events(window_id)
            .unwrap_or_default()
    }

    pub async fn list_windows(&self) -> Vec<WindowInfo> {
        let backend_windows = self.backend.lock().await.list_windows();
        self.windows
            .read()
            .await
            .values()
            .filter(|entry| backend_windows.contains(&entry.id))
            .cloned()
            .collect()
    }

    pub fn host_kind(&self) -> &HostKind {
        self.host.kind()
    }

    async fn allocate_window_id(&self, task_id: u64) -> WindowId {
        let mut next = self.next_window_id.lock().await;
        let local = *next;
        *next = next.saturating_add(1);
        (task_id << 32) | local
    }
}

fn apply_draw_commands(
    text_renderer: &dyn TextRenderer,
    frame: &mut [u8],
    width: u32,
    height: u32,
    commands: &[DrawCommand],
) {
    for command in commands {
        match command {
            DrawCommand::Clear { rgba } => {
                for pixel in frame.chunks_exact_mut(4) {
                    pixel.copy_from_slice(rgba);
                }
            }
            DrawCommand::Pixel { x, y, rgba } => {
                write_pixel(frame, width, *x, *y, *rgba);
            }
            DrawCommand::Text { x, y, text, rgba } => {
                text_renderer.draw_text(frame, width, height, *x, *y, text, *rgba)
            }
        }
    }
}

fn write_pixel(frame: &mut [u8], width: u32, x: u32, y: u32, rgba: [u8; 4]) {
    let idx = ((y * width + x) * 4) as usize;
    if idx + 3 >= frame.len() {
        return;
    }
    frame[idx..idx + 4].copy_from_slice(&rgba);
}
