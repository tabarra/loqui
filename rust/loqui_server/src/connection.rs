use std::sync::Arc;

use futures::sync::mpsc;
use tokio::await as tokio_await;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio_codec::Framed;

use super::{frame_handler::FrameHandler, RequestContext};
use failure::err_msg;
use loqui_protocol::codec::{LoquiCodec, LoquiFrame};
use loqui_protocol::frames::*;

#[derive(Debug)]
enum Message {
    Request(LoquiFrame),
    Response(LoquiFrame),
}

pub struct Connection {
    tcp_stream: TcpStream,
    frame_handler: Arc<dyn FrameHandler>,
    encoding: String,
}

impl Connection {
    pub fn new(tcp_stream: TcpStream, frame_handler: Arc<FrameHandler>) -> Self {
        Self {
            tcp_stream,
            frame_handler,
            // TODO:
            encoding: "json".to_string(),
        }
    }

    pub async fn run<'e>(mut self) {
        self = await!(self.upgrade());
        let framed_socket = Framed::new(self.tcp_stream, LoquiCodec::new(50000 * 1000));
        let (mut writer, mut reader) = framed_socket.split();
        // TODO: handle disconnect

        let (tx, rx) = mpsc::unbounded::<Message>();
        let mut stream = reader
            .map(|frame| Message::Request(frame))
            .select(rx.map_err(|()| err_msg("rx error")));

        while let Some(message) = await!(stream.next()) {
            // TODO: handle error
            match message {
                Ok(message) => {
                    match message {
                        Message::Request(frame) => {
                            let tx = tx.clone();
                            let frame_handler = self.frame_handler.clone();
                            tokio::spawn_async(
                                async move {
                                    // TODO: handle error
                                    match tokio_await!(Box::into_pin(frame_handler.handle_frame(frame))) {
                                        Ok(Some(frame)) => {
                                            tokio_await!(tx.send(Message::Response(frame)));
                                        }
                                        Ok(None) => {
                                            dbg!("None");
                                        }
                                        Err(e) => {
                                            dbg!(e);
                                        }
                                    }
                                },
                            );
                        }
                        Message::Response(frame) => {
                            match tokio_await!(writer.send(frame)) {
                                Ok(new_writer) => writer = new_writer,
                                // TODO: better handle this error
                                Err(e) => {
                                    error!("Failed to write. error={:?}", e);
                                    return;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    dbg!(e);
                }
            }
        }
        println!("connection closed");
    }

    async fn upgrade(mut self) -> Self {
        // TODO: buffering
        let mut payload = [0; 1024];
        // TODO: handle disconnect, bytes_read=0
        while let Ok(_bytes_read) = await!(self.tcp_stream.read_async(&mut payload)) {
            let request = String::from_utf8(payload.to_vec()).unwrap();
            // TODO: better
            if request.contains(&"upgrade") || request.contains(&"Upgrade") {
                let response =
                    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: loqui\r\nConnection: Upgrade\r\n\r\n";
                await!(self.tcp_stream.write_all_async(&response.as_bytes()[..])).unwrap();
                await!(self.tcp_stream.flush_async()).unwrap();
                break;
            }
        }
        self
    }
}
