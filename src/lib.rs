use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use percent_encoding::percent_encode;
use sha2::{Digest, Sha256};
use url::Url;

type HmacSha256 = Hmac<Sha256>;

const LONG_DATETIME_FMT: &str = "%Y%m%dT%H%M%SZ";
const SHORT_DATE_FMT: &str = "%Y%m%d";

const PERCENT_ENCODING_CHARSET: percent_encoding::AsciiSet = percent_encoding::CONTROLS.add(b'/').add(b':').add(b'+');
// Safe characters: https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-keys.html
const S3_KEY_PERCENT_ENCODING_CHARSET: percent_encoding::AsciiSet = percent_encoding::NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'!')
    .remove(b'_')
    .remove(b'.')
    .remove(b'*')
    .remove(b'\'')
    //.remove(b'(') // OCI can't handle this
    //.remove(b')')
    .remove(b'~');

/// AWS Credentials
#[derive(Debug, Clone, PartialEq)]
pub struct Credentials {
    /// AWS_ACCESS_KEY_ID,
    /// The access key applications use for authentication
    access_key: String,
    /// AWS_SECRET_ACCESS_KEY
    /// The secret key applications use for authentication
    secret_key: String,
    /// AWS_SESSION_TOKEN
    // ref: https://docs.aws.amazon.com/STS/latest/APIReference/CommonParameters.html
    /// The session token applications use for authentication, temporary credentials
    session_token: Option<String>,
}

impl Credentials {
    pub fn new(access_key: &str, secret_key: &str, session_token: Option<&str>) -> Self {
        Self {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            session_token: session_token.map(|s| s.into()),
        }
    }

    pub fn new_temporary(access_key: &str, secret_key: &str, session_token: &str) -> Self {
        Self {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            session_token: Some(session_token.into()),
        }
    }
}

/// S3 Bucket
#[derive(Debug, Clone)]
pub struct Bucket {
    /// AWS_DEFAULT_REGION, AWS_REGION
    region: String,
    bucket: String,

    root: String,
}

impl Bucket {
    pub fn new(region: &str, bucket: &str) -> Self {
        Self {
            region: region.into(),
            bucket: bucket.into(),
            root: "s3.amazonaws.com".into(),
        }
    }

    pub fn new_with_root(region: &str, bucket: &str, root: &str) -> Self {
        Self {
            region: region.into(),
            bucket: bucket.into(),
            root: root.into(),
        }
    }

    pub fn from_with_root(s: &str, root: &str) -> Self {
        if s.contains(":") {
            let mut parts = s.splitn(2, ':');
            let region = parts.next().unwrap();
            let bucket = parts.next().unwrap();
            Self {
                region: region.into(),
                bucket: bucket.into(),
                root: root.into(),
            }
        } else {
            Self {
                region: "us-east-1".into(),
                bucket: s.into(),
                root: root.into(),
            }
        }
    }
}

impl From<&str> for Bucket {
    fn from(s: &str) -> Self {
        Bucket::from_with_root(s, "s3.amazonaws.com")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AddressingStyle {
    Virtual,
    Path,
}

/// Generate a presigned URL
#[derive(Debug)]
pub struct Presigner {
    credentials: Credentials,
    bucket: String,
    root: String,
    region: String,
    endpoint: Url,
    addressing_style: AddressingStyle,
}

impl Presigner {
    pub fn new(cred: Credentials, bucket: &str, region: &str) -> Self {
        Self::new_with_root(cred, bucket, region, "s3.amazonaws.com")
    }

    pub fn new_with_root(cred: Credentials, bucket: &str, region: &str, root: &str) -> Self {
        Self {
            credentials: cred,
            bucket: bucket.into(),
            root: root.into(),
            region: region.into(),
            endpoint: Url::parse(&format!("https://{}.{}", bucket, root)).unwrap(),
            addressing_style: AddressingStyle::Virtual,
        }
    }

    pub fn from_bucket(credentials: Credentials, bucket: &Bucket) -> Self {
        Self::new_with_root(credentials, bucket.bucket.as_str(), bucket.region.as_str(), bucket.root.as_str())
    }

    /// Set the endpoint to use for presigned URLs, also enables path style
    pub fn endpoint<U: TryInto<Url>>(&mut self, url: U) -> &mut Self
    where
        <U as TryInto<Url>>::Error: core::fmt::Debug,
    {
        self.endpoint = url.try_into().unwrap();
        self.addressing_style = AddressingStyle::Path;
        self
    }

    pub fn use_path_style(&mut self) -> &mut Self {
        self.addressing_style = AddressingStyle::Path;
        if self.endpoint == Url::parse(&format!("https://{}.{}", self.bucket, self.root)).unwrap() {
            self.endpoint = Url::parse(&format!("https://{}/{}", self.root, self.bucket)).unwrap();
        }
        self
    }

    /// Convert from s3://bucket/key URL to http url
    pub fn url_for_s3_url(&self, url: &Url) -> Option<Url> {
        if url.scheme() != "s3" {
            return None;
        }
        let bucket = url.host_str()?;
        let key = url.path().trim_start_matches('/');

        // S3 has special percent encoding rules for keys
        // let key = percent_encoding::percent_decode_str(&key);
        // let key = escape_key(&key.decode_utf8().ok()?);

        match self.addressing_style {
            AddressingStyle::Virtual => {
                if bucket != self.bucket {
                    return None;
                }
                self.endpoint.join(&key).ok()
            }
            AddressingStyle::Path => {
                let endpoint = self.endpoint.clone();
                endpoint.join(&(bucket.to_owned() + "/")).unwrap().join(key).ok()
            }
        }
    }

    pub fn url_for_key(&self, key: &str) -> Option<Url> {
        if self.bucket.is_empty() {
            return None;
        }
        match self.addressing_style {
            AddressingStyle::Virtual => self.endpoint.join(key).ok(),
            AddressingStyle::Path => {
                let mut endpoint = self.endpoint.clone();
                endpoint.set_path(&format!("{}/{}", self.bucket, key));
                Some(endpoint)
            }
        }
    }

    pub fn get(&self, key: &str, expires: i64) -> Option<String> {
        let url = self.url_for_key(key)?;
        let now = Utc::now();
        presigned_url(
            &self.credentials,
            expires as _,
            &url,
            "GET",
            "UNSIGNED-PAYLOAD",
            &self.region,
            &now,
            "s3",
            vec![],
        )
    }

    pub fn put(&self, key: &str, expires: i64) -> Option<String> {
        let url = self.url_for_key(key)?;
        let now = Utc::now();
        presigned_url(
            &self.credentials,
            expires as _,
            &url,
            "PUT",
            "UNSIGNED-PAYLOAD",
            &self.region,
            &now,
            "s3",
            vec![],
        )
    }

    pub fn url_join(&self, key: &str) -> Option<Url> {
        self.url_for_key(key)
    }

    pub fn sign_request(
        &self,
        method: &str,
        url: &Url,
        expiration: u64,
        extra_headers: Vec<(String, String)>,
    ) -> Option<String> {
        let now = Utc::now();
        presigned_url(
            &self.credentials,
            expiration,
            url,
            method,
            "UNSIGNED-PAYLOAD",
            &self.region,
            &now,
            "s3",
            extra_headers,
        )
    }
}

/// Generate a presigned GET URL for downloading
pub fn get(credentials: &Credentials, bucket: &Bucket, key: &str, expires: i64) -> Option<String> {
    let url = format!("https://{}.{}/{}", bucket.bucket, bucket.root, escape_key(key));
    let now = Utc::now();

    presigned_url(
        &credentials,
        expires as _,
        &url.parse().unwrap(),
        "GET",
        "UNSIGNED-PAYLOAD",
        &bucket.region,
        &now,
        "s3",
        vec![],
    )
}

/// Generate a presigned PUT URL for uploading
pub fn put(credentials: &Credentials, bucket: &Bucket, key: &str, expires: i64) -> Option<String> {
    let url = format!("https://{}.{}/{}", bucket.bucket, bucket.root, escape_key(key));
    /*let url = format!(
        "https://s3.amazonaws.com/{}/{}",
        bucket.bucket,
        escape_key(key)
    );*/
    let now = Utc::now();

    presigned_url(
        credentials,
        expires as _,
        &url.parse().unwrap(),
        "PUT",
        "UNSIGNED-PAYLOAD",
        &bucket.region,
        &now,
        "s3",
        vec![],
    )
}

fn escape_key(key: &str) -> String {
    let mut encoded = true;
    for (i, &c) in key.as_bytes().iter().enumerate() {
        if c == b'%' {
            if i + 2 >= key.len() {
                encoded = false;
                break;
            }
            let c1 = key.as_bytes()[i + 1];
            let c2 = key.as_bytes()[i + 2];
            if !matches!(c1, b'a'..=b'f' | b'A'..=b'F' | b'0'..=b'9') {
                encoded = false;
                break;
            }
            if !matches!(c2, b'a'..=b'f' | b'A'..=b'F' | b'0'..=b'9') {
                encoded = false;
                break;
            }
        }
        if !matches!(c, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'/' | b',') {
            encoded = false;
            break;
        }
    }
    if encoded {
        key.into() // assume esacped
    } else {
        percent_encode(key.as_bytes(), &S3_KEY_PERCENT_ENCODING_CHARSET).to_string()
    }
}

/// Generate pre-signed s3 URL
pub fn presigned_url(
    credentials: &Credentials,
    expiration: u64,
    url: &Url,
    method: &str,
    payload_hash: &str,
    region: &str,
    date_time: &DateTime<Utc>,
    service: &str,
    extra_headers: Vec<(String, String)>,
) -> Option<String> {
    let access_key = &credentials.access_key;
    let secret_key = &credentials.secret_key;
    let session_token = credentials.session_token.as_ref();

    let date_time_txt = date_time.format(LONG_DATETIME_FMT).to_string();
    let short_date_time_txt = date_time.format(SHORT_DATE_FMT).to_string();
    let credentials = format!(
        "{}/{}/{}/{}/aws4_request",
        access_key, short_date_time_txt, region, service
    );
    let mut params = vec![
        ("X-Amz-Algorithm".to_string(), "AWS4-HMAC-SHA256".to_string()),
        ("X-Amz-Credential".to_string(), credentials),
        ("X-Amz-Date".to_string(), date_time_txt),
        // only relevant for the S3 service
        // Ref: https://github.com/aws/aws-sdk-go/issues/2167#issuecomment-430792002
        ("X-Amz-Expires".to_string(), expiration.to_string()),
        ("X-Amz-SignedHeaders".to_string(), "host".to_string()),
    ];
    for (k, v) in extra_headers {
        params.push((k, v));
    }
    if let Some(session_token) = session_token {
        params.push(("X-Amz-Security-Token".to_string(), session_token.to_string()));
    }

    url.query_pairs().for_each(|(k, v)| {
        params.push((k.to_string(), v.to_string()));
    });

    params.sort();

    let canonical_query_string = params
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                percent_encode(k.as_bytes(), &PERCENT_ENCODING_CHARSET),
                percent_encode(v.as_bytes(), &PERCENT_ENCODING_CHARSET)
            )
        })
        .collect::<Vec<_>>()
        .join("&");

    // NOTE: this is not the same as the canonical query string
    let query_keys = url.query_pairs().map(|(k, _)| k.to_string()).collect::<Vec<_>>();
    let query_string = if query_keys.is_empty() {
        canonical_query_string.clone()
    } else {
        params
            .iter()
            .filter(|(k, _)| !query_keys.contains(k))
            .map(|(k, v)| {
                format!(
                    "{}={}",
                    percent_encode(k.as_bytes(), &PERCENT_ENCODING_CHARSET),
                    percent_encode(v.as_bytes(), &PERCENT_ENCODING_CHARSET)
                )
            })
            .collect::<Vec<_>>()
            .join("&")
    };

    let canonical_resource = url.path();

    let mut host = url.host_str().unwrap().to_owned();
    if let Some(port) = url.port() {
        host.push(':');
        host.push_str(&port.to_string());
    }

    let canonical_headers = format!("host:{}", host);
    let signed_headers = "host";
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n\n{}\n{}",
        method.to_uppercase(),
        canonical_resource,
        canonical_query_string,
        canonical_headers,
        signed_headers,
        payload_hash
    );
    let string_to_sign = string_to_sign(&date_time, &region, &canonical_request, service);
    let signing_key = signing_key(&date_time, secret_key, region, service)?;

    let mut hmac = HmacSha256::new_from_slice(&signing_key).ok()?;
    hmac.update(string_to_sign.as_bytes());
    let signature = format!("{:x}", hmac.finalize().into_bytes());

    let request_url = if url.query().is_some() {
        url.to_string() + "&" + &query_string + "&X-Amz-Signature=" + &signature
    } else {
        url.to_string() + "?" + &query_string + "&X-Amz-Signature=" + &signature
    };

    Some(request_url)
}

/// Generate the "string to sign" - the value to which the HMAC signing is
/// applied to sign requests.
fn string_to_sign(date_time: &DateTime<Utc>, region: &str, canonical_req: &str, service: &str) -> String {
    let mut hasher = Sha256::default();
    hasher.update(canonical_req.as_bytes());
    format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{hash}",
        timestamp = date_time.format(LONG_DATETIME_FMT),
        scope = scope_string(date_time, region, service),
        hash = format!("{:x}", hasher.finalize())
    )
}

/// Generate the AWS signing key, derived from the secret key, date, region,
/// and service name.
fn signing_key(date_time: &DateTime<Utc>, secret_key: &str, region: &str, service: &str) -> Option<Vec<u8>> {
    let secret = format!("AWS4{}", secret_key);
    let mut date_hmac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    date_hmac.update(date_time.format(SHORT_DATE_FMT).to_string().as_bytes());
    let mut region_hmac = HmacSha256::new_from_slice(&date_hmac.finalize().into_bytes()).ok()?;
    region_hmac.update(region.to_string().as_bytes());
    let mut service_hmac = HmacSha256::new_from_slice(&region_hmac.finalize().into_bytes()).ok()?;
    service_hmac.update(service.as_bytes());
    let mut signing_hmac = HmacSha256::new_from_slice(&service_hmac.finalize().into_bytes()).ok()?;
    signing_hmac.update(b"aws4_request");
    Some(signing_hmac.finalize().into_bytes().to_vec())
}

/// Generate an AWS scope string.
fn scope_string(date_time: &DateTime<Utc>, region: &str, service: &str) -> String {
    format!(
        "{date}/{region}/{service}/aws4_request",
        date = date_time.format(SHORT_DATE_FMT),
        region = region,
        service = service
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate() {
        let credentials = Credentials {
            access_key: "ASIAAAAAABBBBBCCCCCDDDDDD".into(),
            secret_key: "AAAAAAA+BBBBBBBB/CCCCCCC/DDDDDDDDDD".into(),
            session_token: Some("xxxxxxxxx".into()),
        };

        let bucket = Bucket {
            region: "us-east-1".into(),
            bucket: "the-bucket".into(),
            root: "s3.amazonaws.com".into(),
        };

        let s = put(
            &credentials,
            &bucket,
            "5e4ed04f-1d37-4cef-8210-eea624f2aef5/f219644fdfb",
            600,
        );
        assert!(s.is_some());
        println!("=> {:?}", s);
    }
}
