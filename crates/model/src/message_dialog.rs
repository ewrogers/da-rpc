#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MessageDialogsState {
    pub revision: u32,
    pub dialogs: Vec<MessageDialog>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageDialog {
    pub id: u32,
    pub text: Option<String>,
    pub truncated: bool,
}
