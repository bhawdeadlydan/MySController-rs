use actix_web::{
    error, multipart, AsyncResponder, Error, FutureResponse, HttpMessage, HttpRequest,
    HttpResponse, Json,
};
use api::index::AppState;
use handler::firmware::*;
use http::StatusCode;
use model::firmware::Firmware;

use bytes::Bytes;
use futures::future;
use futures::{Future, Stream};
use ihex::record::Record;
use std;

pub fn upload_form(_req: HttpRequest<AppState>) -> Result<HttpResponse, error::Error> {
    let html = r#"<html>
        <head><title>Upload Test</title></head>
        <body>
            <form action="/firmwares" method="post" enctype="multipart/form-data">
                Name: <input type="text" name="firmware_name"/><br>
                Type: <input type="text" name="firmware_type"/><br>
                Version: <input type="text" name="firmware_version"/><br>
                <input type="file" name="firmware_file"/><br>
                <input type="submit" value="Submit"/>
            </form>
        </body>
    </html>"#;

    Ok(HttpResponse::Ok().body(html))
}

pub fn list(req: HttpRequest<AppState>) -> FutureResponse<HttpResponse> {
    req.state()
        .db
        .send(ListFirmwares)
        .from_err()
        .and_then(|res| match res {
            Ok(msg) => Ok(HttpResponse::Ok().json(msg)),
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder()
}

pub fn delete(
    firmware_delete: Json<DeleteFirmware>,
    req: HttpRequest<AppState>,
) -> FutureResponse<HttpResponse> {
    req.state()
        .db
        .send(DeleteFirmware {
            firmware_type: firmware_delete.firmware_type,
            firmware_version: firmware_delete.firmware_version,
        })
        .from_err()
        .and_then(|res| match res {
            Ok(msg) => Ok(HttpResponse::build(StatusCode::from_u16(msg.status).unwrap()).json(msg)),
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder()
}

pub fn create(req: HttpRequest<AppState>) -> FutureResponse<HttpResponse> {
    let req_clone = req.clone();
    req_clone
        .multipart()
        .map_err(error::ErrorInternalServerError)
        .map(handle_multipart_item)
        .flatten()
        .collect()
        .and_then(move |fields| {
            req.state()
                .db
                .send(create_firmware(fields))
                .from_err()
                .and_then(|res| match res {
                    Ok(msg) => Ok(
                        HttpResponse::build(StatusCode::from_u16(msg.status).unwrap()).json(msg),
                    ),
                    Err(e) => {
                        println!("{:?}", e);
                        Ok(HttpResponse::InternalServerError().into())
                    }
                })
                .responder()
        })
        .responder()
}

fn extract_single_field(
    fields: &Vec<(Option<String>, Option<Vec<Bytes>>)>,
    field_name: &str,
) -> Vec<Bytes> {
    fields
        .into_iter()
        .find(|(field_header, _field_value)| match field_header {
            Some(ref header) => header.contains(field_name),
            None => false,
        })
        .map(|(_field_header, field_value)| match field_value {
            Some(value) => value.to_owned(),
            None => Vec::new(),
        })
        .unwrap_or(Vec::new())
}

fn create_firmware(fields: Vec<(Option<String>, Option<Vec<Bytes>>)>) -> NewFirmware {
    let firmware_name = extract_single_field(&fields, "firmware_name");
    let firmware_type = extract_single_field(&fields, "firmware_type");
    let firmware_version = extract_single_field(&fields, "firmware_version");
    let firmware_file = extract_single_field(&fields, "firmware_file");

    let firmware_name = to_string(firmware_name);
    let firmware_type = to_string(firmware_type);
    let firmware_version = to_string(firmware_version);
    let binary_data: Vec<u8> = firmware_file
        .into_iter()
        .map(|b| {
            String::from(std::str::from_utf8(b.as_ref()).unwrap())
                .trim()
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .flat_map(|line| Firmware::ihex_to_bin(&Record::from_record_string(&line).unwrap()))
        .collect();

    let new_firmware = NewFirmware::prepare_in_memory(
        firmware_type.parse::<i32>().unwrap(),
        firmware_version.parse::<i32>().unwrap(),
        firmware_name,
        binary_data,
    );
    println!("new_firmware {:?}", new_firmware);
    new_firmware
}

fn handle_multipart_item(
    item: multipart::MultipartItem<HttpRequest<AppState>>,
) -> Box<Stream<Item = (Option<String>, Option<Vec<Bytes>>), Error = Error>> {
    match item {
        multipart::MultipartItem::Field(field) => {
            Box::new(extract_field_value(field).into_stream())
        }
        multipart::MultipartItem::Nested(mp) => Box::new(
            mp.map_err(error::ErrorInternalServerError)
                .map(handle_multipart_item)
                .flatten(),
        ),
    }
}

fn extract_field_value(
    field: multipart::Field<HttpRequest<AppState>>,
) -> Box<Future<Item = (Option<String>, Option<Vec<Bytes>>), Error = Error>> {
    println!("field: {:?}", field);
    Box::new(future::result(Ok((
        content_disposition(&field),
        field
            .map_err(|e| error::ErrorInternalServerError(e))
            .collect()
            .wait()
            .ok(),
    ))))
}

fn content_disposition(field: &multipart::Field<HttpRequest<AppState>>) -> Option<String> {
    // RFC 7578: 'Each part MUST contain a Content-Disposition header field
    // where the disposition type is "form-data".'
    field
        .headers()
        .get(::http::header::CONTENT_DISPOSITION)
        .and_then(|f| f.to_str().map(|string| string.to_owned()).ok())
}

fn to_string(value: Vec<Bytes>) -> String {
    String::from(
        std::str::from_utf8(
            value
                .into_iter()
                .flat_map(|b| b.as_ref().to_owned())
                .collect::<Vec<u8>>()
                .as_ref(),
        ).unwrap(),
    )
}
