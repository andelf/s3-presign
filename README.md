# s3-presign

Minimal library to get an S3 presign URL.

## Usage

```rust
use s3_presign::{Credentials, Presigner};

let credentials = Credentials::new(
    "blah.....blah...",
    "blah.....xxxxxxx",
    None,
);

let bucket = "my-bucket";
let region = "us-east-1";

let mut presigner = Presigner::new(credentials, bucket, region);
// presigner.endpoint(endpoint);
let url = self.presigner.url_for_s3_url(path).unwrap();

let signed_url = self.sign_s3_request("HEAD", &url);

let signed_url = self.sign_s3_request("PUT", &url);
let signed_url = self.sign_s3_request("GET", &url);
```
