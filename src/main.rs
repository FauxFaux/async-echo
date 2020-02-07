use async_std::net::TcpListener;
use async_std::net::TcpStream;
use async_std::task;
use futures::future::FusedFuture as _;
use futures::io::AsyncReadExt as _;
use futures::io::AsyncWriteExt as _;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt as _;
use futures::FutureExt as _;

fn main() {
    pretty_env_logger::init();
    task::block_on(run());
    log::info!("app exiting");
}

async fn run() {
    let serv = TcpListener::bind("localhost:1773").await.expect("bind");
    log::info!("listening on localhost:1773");

    let mut incoming = serv.incoming().fuse();
    let mut stopper = async_ctrlc::CtrlC::new().expect("init").fuse();

    let mut workers = FuturesUnordered::new();

    loop {
        let client = futures::select! {
            _ = stopper => {
                log::info!("woken by cancellation");
                break;
            },
            client = incoming.next() => client,
            garbage = workers.next() => {
                // unfortunately, we see the None here
                if let Some(()) = garbage {
                    log::debug!("garbage collected a client");
                }
                continue;
            },
        };

        // I think this might be irrefutable.
        let client = match client {
            Some(client) => client,
            None => break,
        };

        let client = client.expect("accept");
        log::info!("accepted client from {:?}", client.peer_addr());
        let handle = client_loop(client);
        workers.push(handle);
    }

    drop(serv);
    log::info!("listener shut down, draining clients");

    while !stopper.is_terminated() {
        futures::select! {
            worker = workers.next() => {
                log::info!("{} clients still connected", workers.len());
                if worker.is_none() {
                    log::info!("all clients disconnected");
                    break;
                }
            },
            _ = stopper => break,
            complete => break,
        }
    }

    log::info!("bye");
}

async fn client_loop(mut conn: TcpStream) {
    let mut buf = [0u8; 4096];
    while let Ok(found) = conn.read(&mut buf).await {
        let buf = &buf[..found];
        if buf.is_empty() {
            log::info!("{:?}: eof", conn.peer_addr());
            break;
        }
        if let Err(e) = conn.write_all(buf).await {
            log::warn!("{:?} couldn't write: {:?}", conn.peer_addr(), e);
            break;
        }
        log::debug!("{:?} wrote {}", conn.peer_addr(), buf.len());
    }
    log::info!("{:?} dropping client", conn.peer_addr());
}
