use serenity::model::{
    guild::{Member, PartialMember},
    user::User,
};

pub trait ReadName {
    fn read_name(&self) -> String;
}

impl ReadName for Member {
    fn read_name(&self) -> String {
        self.nick
            .as_ref()
            .map(|n| n.to_string())
            .unwrap_or_else(|| self.display_name().to_string())
    }
}

impl ReadName for PartialMember {
    fn read_name(&self) -> String {
        self.nick
            .as_ref()
            .map(|n| n.to_string())
            .unwrap_or_else(|| self.user.as_ref().unwrap().display_name().to_string())
    }
}

impl ReadName for User {
    fn read_name(&self) -> String {
        self.display_name().to_string()
    }
}
