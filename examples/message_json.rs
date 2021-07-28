use rngrok::{msg::*, util};

fn main() -> anyhow::Result<()> {
    let msg = Envelope::from(RegProxy {
        client_id: util::rand_id(8),
    });
    let json = serde_json::to_string(&msg)?;
    println!("{}", &json);
    let msg = serde_json::from_str::<Envelope>(&json)?;
    println!("{:?}", msg);
    let p = Message::from_str(&json)?;
    println!("{:?}", p);
    Ok(())
}
