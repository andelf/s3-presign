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

## S3-compatible APIs
If you want to use the presigner with a different service that exposes an S3-compatible API (like Cloudflare R2),
you can do so by specifying a `root` when creating the `Presigner`/`Bucket`.

```rust
let root = "xxxxx.eu.r2.cloudflarestorage.com";

let bucketObj = Bucket::mew_with_root(bucket, root, root);
let mut preBucket = Presigner::from_bucket(credentials, bucketObj);
preBucket.use_path_style(); // Cloudflare R2 only works with Path-Style!!!

let mut presigner = Presigner::new_with_root(credentials, bucket, region, root);
```

> ⚠️ Cloudflare R2
> 
> The R2s S3-API requires path style! Therefore, you can not use the global convenience function.
> Instead you need to create a `Presigner` and call `Presigner::use_path_style(&self)` on it.
