use serenity::model::guild::Member;

pub trait ReadName {
    fn read_name(&self) -> String;
}

impl ReadName for Member {
    fn read_name(&self) -> String {
        self.nick.clone().unwrap_or(self.user.name.clone())
    }
}
