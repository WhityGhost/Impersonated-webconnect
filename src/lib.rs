use futures_util::{stream::{SplitSink, SplitStream}, SinkExt, StreamExt, TryStreamExt};
use neon::prelude::*;
use reqwest_impersonate::{self as reqwest, WebSocket, Message};
use reqwest::{impersonate::Impersonate, Client};
use tokio::runtime::Runtime;
use std::{borrow::BorrowMut, error::Error, sync::Arc};


#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("connect", connect)?;
    cx.export_function("send_message", send_message)?;
    cx.export_function("receive_message", receive_message)?;
    Ok(())
}

struct WebSocketClient  {
    url: String,
    tx: Option<SplitSink<WebSocket, Message>>,
    rx: Option<SplitStream<WebSocket>>,
}

impl WebSocketClient {
    fn new() -> Self {
        WebSocketClient {
            url: "".to_string(),
            tx: None,
            rx: None
        }
    }
    
    #[tokio::main]
    pub async fn connect(&mut self, url: String) -> Result<bool, Box<dyn Error>> {
        let websocket = Client::builder()
            .impersonate_websocket(Impersonate::Chrome126)
            .build()?
            .get(url.clone())
            .upgrade()
            .send()
            .await?
            .into_websocket()
            .await?;
        self.url = url;

        let (tx, rx) = websocket.split();

        self.tx = Some(tx);
        self.rx = Some(rx);

        Ok(true)
    }

    pub fn send_message(&mut self, msg: String) -> bool {
        if let Some(mut sender) = self.tx.take() {
            sender.send(Message::Text(msg));
        } else {
            return false;
        }
        true
    }

    pub async fn receive_message(&mut self) -> Result<String, Box<dyn Error>> {
        if let Some(mut receiver) = self.rx.take() {
            if let Some(message) = receiver.try_next().await? {
                match message {
                    Message::Text(text) => {
                        return Ok(text);
                    },
                    _ => {return Ok("".to_string())}
                };
            }
        }
        Ok("".to_string())
    }
}

impl Finalize for WebSocketClient {}

fn connect(mut cx: FunctionContext) -> JsResult<JsBox<WebSocketClient>> {
    let url = cx.argument::<JsString>(0)?.value(&mut cx);
    let mut client = WebSocketClient::new();
    client.connect(url);
    Ok(cx.boxed(client))
}

fn send_message(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let message = cx.argument::<JsString>(0)?.value(&mut cx);
    let handle = cx.this_value().downcast_or_throw::<JsBox<WebSocketClient>, _>(&mut cx)?;
    let client = handle.clone();

    let (deferred, promise) = cx.promise();
    let channel = cx.channel(); 

    tokio::task::spawn_blocking(move || {
        let rt = Runtime::new().unwrap();
        let res = rt.block_on(async {
            client.send_message(message)
        });

        channel.send(move |mut cx| {
            if res { deferred.resolve(&mut cx, cx.boolean(true)); }
            else { deferred.reject(&mut cx, cx.error("[Socket Error] Failed to send a message.").unwrap()); }
            Ok(())
        });
    });

    Ok(promise)
}

fn receive_message(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let handle = cx.this_value().downcast_or_throw::<JsBox<WebSocketClient>, _>(&mut cx)?;
    let client = &**handle;

    let client = client.clone();
    let promise = cx
        .task(move || async {
            client.receive_message().await
        })
        .promise(|mut cx, res| async {
            match res.await {
                Ok(msg) => return Ok(cx.string(msg)),
                Err(err) => return Err(err),
            }
        })

    Ok(promise)
}