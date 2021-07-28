use serde::{Deserialize, Serialize};
use serde_json::{from_str, from_value, to_value, Value};

#[derive(Serialize, Deserialize, Debug)]
pub struct Envelope {
    #[serde(rename = "Type")]
    pub r#type: String,
    #[serde(rename = "Payload")]
    pub payload: Value,
}

impl From<Auth> for Envelope {
    fn from(msg: Auth) -> Self {
        Envelope {
            r#type: "Auth".to_string(),
            payload: to_value(msg).unwrap(),
        }
    }
}
impl From<AuthResp> for Envelope {
    fn from(msg: AuthResp) -> Self {
        Envelope {
            r#type: "AuthResp".to_string(),
            payload: to_value(msg).unwrap(),
        }
    }
}
impl From<ReqTunnel> for Envelope {
    fn from(msg: ReqTunnel) -> Self {
        Envelope {
            r#type: "ReqTunnel".to_string(),
            payload: to_value(msg).unwrap(),
        }
    }
}
impl From<NewTunnel> for Envelope {
    fn from(msg: NewTunnel) -> Self {
        Envelope {
            r#type: "NewTunnel".to_string(),
            payload: to_value(msg).unwrap(),
        }
    }
}
impl From<ReqProxy> for Envelope {
    fn from(msg: ReqProxy) -> Self {
        Envelope {
            r#type: "ReqProxy".to_string(),
            payload: to_value(msg).unwrap(),
        }
    }
}
impl From<RegProxy> for Envelope {
    fn from(msg: RegProxy) -> Self {
        Envelope {
            r#type: "RegProxy".to_string(),
            payload: to_value(msg).unwrap(),
        }
    }
}
impl From<StartProxy> for Envelope {
    fn from(msg: StartProxy) -> Self {
        Envelope {
            r#type: "StartProxy".to_string(),
            payload: to_value(msg).unwrap(),
        }
    }
}
impl From<Ping> for Envelope {
    fn from(msg: Ping) -> Self {
        Envelope {
            r#type: "Ping".to_string(),
            payload: to_value(msg).unwrap(),
        }
    }
}
impl From<Pong> for Envelope {
    fn from(msg: Pong) -> Self {
        Envelope {
            r#type: "Pong".to_string(),
            payload: to_value(msg).unwrap(),
        }
    }
}

#[derive(Debug)]
pub enum Message {
    Auth(Auth),
    AuthResp(AuthResp),
    ReqTunnel(ReqTunnel),
    NewTunnel(NewTunnel),
    ReqProxy(ReqProxy),
    RegProxy(RegProxy),
    StartProxy(StartProxy),
    Ping(Ping),
    Pong(Pong),
}

impl Message {
    pub fn from_str(s: &str) -> anyhow::Result<Message> {
        let msg = from_str::<Envelope>(s)?;
        match msg.r#type.as_str() {
            "Auth" => Ok(Message::Auth(from_value::<Auth>(msg.payload)?)),
            "AuthResp" => Ok(Message::AuthResp(from_value::<AuthResp>(msg.payload)?)),
            "ReqTunnel" => Ok(Message::ReqTunnel(from_value::<ReqTunnel>(msg.payload)?)),
            "NewTunnel" => Ok(Message::NewTunnel(from_value::<NewTunnel>(msg.payload)?)),
            "ReqProxy" => Ok(Message::ReqProxy(from_value::<ReqProxy>(msg.payload)?)),
            "RegProxy" => Ok(Message::RegProxy(from_value::<RegProxy>(msg.payload)?)),
            "StartProxy" => Ok(Message::StartProxy(from_value::<StartProxy>(msg.payload)?)),
            "Ping" => Ok(Message::Ping(from_value::<Ping>(msg.payload)?)),
            "Pong" => Ok(Message::Pong(from_value::<Pong>(msg.payload)?)),
            _ => Err(anyhow::anyhow!("Unsupported message type {}", msg.r#type)),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Auth {
    #[serde(rename = "Version")]
    pub version: String, // protocol version
    #[serde(rename = "MmVersion")]
    pub mm_version: String, // major/minor software version (informational only)
    #[serde(rename = "User")]
    pub user: String,
    #[serde(rename = "Password")]
    pub password: String,
    #[serde(rename = "OS")]
    pub os: String,
    #[serde(rename = "Arch")]
    pub arch: String,
    #[serde(rename = "ClientId")]
    pub client_id: String, // empty for new sessions
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct AuthResp {
    #[serde(rename = "Version")]
    pub version: Option<String>,
    #[serde(rename = "MmVersion")]
    pub mm_version: Option<String>,
    #[serde(rename = "ClientId")]
    pub client_id: Option<String>,
    #[serde(rename = "Error")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReqTunnel {
    #[serde(rename = "ReqId")]
    pub req_id: String,
    #[serde(rename = "Protocol")]
    pub protocol: String,

    // http only
    #[serde(rename = "Hostname")]
    pub hostname: Option<String>,
    #[serde(rename = "Subdomain")]
    pub subdomain: Option<String>,
    #[serde(rename = "HttpAuth")]
    pub http_auth: String,

    // tcp only
    #[serde(rename = "RemotePort")]
    pub remote_port: Option<u16>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct NewTunnel {
    #[serde(rename = "ReqId")]
    pub req_id: Option<String>,
    #[serde(rename = "Url")]
    pub url: Option<String>,
    #[serde(rename = "Protocol")]
    pub protocol: Option<String>,
    #[serde(rename = "Error")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReqProxy {}

#[derive(Serialize, Deserialize, Debug)]
pub struct RegProxy {
    #[serde(rename = "ClientId")]
    pub client_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StartProxy {
    #[serde(rename = "Url")]
    pub url: String, // URL of the tunnel this connection connection is being proxied for
    #[serde(rename = "ClientAddr")]
    pub client_addr: String, // Network address of the client initiating the connection to the tunnel
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Ping {}

#[derive(Serialize, Deserialize, Debug)]
pub struct Pong {}
