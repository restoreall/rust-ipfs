use futures::stream::{TryStream, TryStreamExt};
use std::error::Error as StdError;
use warp::http::header::{HeaderValue, CONTENT_TYPE, TRAILER};
use warp::http::Response;
use warp::hyper::body::Bytes;
use warp::hyper::Body;
use warp::Reply;

pub struct StreamResponseJson<S>(pub S);
pub struct StreamResponseText<S>(pub S);

impl<S> Reply for StreamResponseJson<S>
where
    S: TryStream + Send + 'static,
    S::Ok: Into<Bytes>,
    S::Error: StdError + Send + Sync + 'static,
{
    fn into_response(self) -> warp::reply::Response {
        inner_into_response(self.0,"application/json")
    }

}

impl<S> Reply for StreamResponseText<S>
where
    S: TryStream + Send + 'static,
    S::Ok: Into<Bytes>,
    S::Error: StdError + Send + Sync + 'static,
{
    fn into_response(self) -> warp::reply::Response {
        inner_into_response(self.0,"text/plain")
    }

}

fn inner_into_response<S>(stream:S,content_type:&'static str)->warp::reply::Response
where
    S: TryStream + Send + 'static,
    S::Ok: Into<Bytes>,
    S::Error: StdError + Send + Sync + 'static,
{
    let mut resp = Response::new(Body::wrap_stream(stream.into_stream()));
    let headers = resp.headers_mut();
    headers.insert(TRAILER, HeaderValue::from_static("X-Stream-Error"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert("X-Chunked-Output", HeaderValue::from_static("1"));
    resp
}