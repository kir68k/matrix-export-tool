pub mod key_import;
pub mod verification;

use std::collections::HashSet;

use crate::ui::ExportApp;
use gpui::{AnyView, App, Context, SharedString, WeakEntity, Window};

pub struct UserTodoTasks {
    tasks: Vec<TodoTaskItem>,
    completed: HashSet<&'static str>,
}

impl Default for UserTodoTasks {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for UserTodoTasks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserTodoTasks")
            .field("tasks_count", &self.tasks.len())
            .field("completed_count", &self.completed.len())
            .finish()
    }
}

impl UserTodoTasks {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            completed: HashSet::new(),
        }
    }

    pub fn add_task<T: TodoTaskBehavior>(&mut self, task: T) {
        self.tasks.push(TodoTaskItem::new(task));
    }

    pub fn tasks(&self) -> &[TodoTaskItem] {
        &self.tasks
    }

    pub fn get(&self, ix: usize) -> Option<&TodoTaskItem> {
        self.tasks.get(ix)
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Mark a task as completed by its ID.
    pub fn mark_completed(&mut self, id: &'static str) {
        self.completed.insert(id);
    }

    /// Check if a task ID has been marked as completed.
    pub fn is_completed(&self, id: &'static str) -> bool {
        self.completed.contains(id)
    }
}

pub trait TodoTaskBehavior: Send + Sync + 'static {
    /// Unique identifier for this task type.
    fn id(&self) -> &'static str;

    /// Check whether the task was fulfilled.
    fn is_finished(&self, app: &ExportApp, cx: &App) -> bool {
        app.todo_tasks.is_completed(self.id())
    }

    /// Task title
    fn title(&self) -> SharedString;

    /// Short task description
    fn label(&self) -> SharedString;

    /// Create the view for the currently selected task
    /// This can be e.g. a dialog or whatever
    fn create_view(
        &self,
        app: WeakEntity<ExportApp>,
        window: &mut Window,
        cx: &mut Context<ExportApp>,
    ) -> AnyView;
}

pub struct TodoTaskItem {
    behavior: Box<dyn TodoTaskBehavior>,
}

impl std::fmt::Debug for TodoTaskItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TodoTaskItem")
            .field("title", &self.behavior.title())
            .finish()
    }
}

impl TodoTaskItem {
    pub fn new<T: TodoTaskBehavior>(task: T) -> Self {
        Self {
            behavior: Box::new(task),
        }
    }

    pub fn id(&self) -> &'static str {
        self.behavior.id()
    }

    pub fn is_finished(&self, app: &ExportApp, cx: &App) -> bool {
        self.behavior.is_finished(app, cx)
    }

    pub fn title(&self) -> SharedString {
        self.behavior.title()
    }

    pub fn label(&self) -> SharedString {
        self.behavior.label()
    }

    pub fn create_view(
        &self,
        app: WeakEntity<ExportApp>,
        window: &mut Window,
        cx: &mut Context<ExportApp>,
    ) -> AnyView {
        self.behavior.create_view(app, window, cx)
    }
}
