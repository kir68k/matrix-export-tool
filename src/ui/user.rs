use gpui::SharedString;
use zeroize::Zeroizing;

use crate::ui::ExportUser;

/// Trait for any type which can represent a user
///
/// Made for when CLI works again (maybe...??)
pub trait ValidUser {
    fn set_username<U: Into<SharedString>>(&mut self, username: U);
    fn set_password(&mut self, password: String);
}

impl ValidUser for ExportUser {
    fn set_username<U: Into<SharedString>>(&mut self, username: U) {
        self.userid = username.into();
    }

    fn set_password(&mut self, password: String) {
        self.password = Zeroizing::from(password);
    }
}
