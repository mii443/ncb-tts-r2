use serenity::model::{
    user::User,
    guild::{Member, PartialMember}
};

pub trait ReadName {
    fn read_name(&self) -> String;
}

impl ReadName for Member {
    fn read_name(&self) -> String {
        self.nick.clone().unwrap_or(self.display_name().to_string())
    }
}

impl ReadName for PartialMember {
    fn read_name(&self) -> String {
        self.nick.clone().unwrap_or(self.user.as_ref().unwrap().display_name().to_string())
    }
}

impl ReadName for User {
    fn read_name(&self) -> String {
        self.display_name().to_string()
    }
}