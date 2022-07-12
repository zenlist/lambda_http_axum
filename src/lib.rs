use axum::response::IntoResponse;
use http::Request;
use lambda_http::Service;
use std::{convert::Infallible, error::Error, future::Future, pin::Pin};

type LambdaRequest = http::Request<lambda_http::Body>;
type LambdaResponse = lambda_http::Response<lambda_http::Body>;
type LambdaError = Box<dyn std::error::Error + Send + Sync>;
type AxumRequest = http::Request<axum::body::Body>;

#[doc(hidden)]
pub fn service_fn<S>(s: S) -> Adapter<S>
where
    S: Service<AxumRequest, Error = Infallible>,
    S::Response: IntoResponse,
    S::Future: 'static,
{
    Adapter(s)
}

#[doc(hidden)]
pub struct Adapter<S>(S);

pub async fn run<S>(handler: S) -> Result<(), Box<dyn Error + Send + Sync>>
where
    S: Service<AxumRequest, Error = Infallible>,
    S::Response: IntoResponse,
    S::Future: 'static,
{
    let handler = service_fn(handler);
    lambda_http::run(handler).await
}

impl<S> Service<LambdaRequest> for Adapter<S>
where
    S: Service<AxumRequest, Error = Infallible>,
    S::Response: IntoResponse,
    S::Future: 'static,
{
    type Response = LambdaResponse;
    type Error = LambdaError;
    type Future =
        Pin<Box<dyn Future<Output = Result<LambdaResponse, Box<dyn Error + Send + Sync>>>>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self.0.poll_ready(cx) {
            std::task::Poll::Ready(Ok(_)) => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Ready(Err(_)) => unreachable!(),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn call(&mut self, req: LambdaRequest) -> Self::Future {
        let (parts, body) = req.into_parts();
        let bytes = match body {
            lambda_http::Body::Empty => bytes::Bytes::new(),
            lambda_http::Body::Text(text) => bytes::Bytes::copy_from_slice(text.as_bytes()),
            lambda_http::Body::Binary(data) => bytes::Bytes::from(data),
        };
        let axum_body = axum::body::Body::from(bytes);
        let req = Request::from_parts(parts, axum_body);

        let future = self.0.call(req);

        let future = async move {
            let response = future.await.unwrap();
            let response = response.into_response();
            let (parts, body) = response.into_parts();
            let body = hyper::body::to_bytes(body).await?;
            let body = lambda_http::Body::Binary(body.to_vec());
            Ok(LambdaResponse::from_parts(parts, body))
        };

        Box::pin(future)
    }
}
