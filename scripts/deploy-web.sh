#!/usr/bin/env bash
# Build the web app and publish it to https://cad.greendealer.co.nz
# (private S3 bucket behind CloudFront). Needs the AWS CLI with credentials
# for the account that owns the bucket and distribution.
set -euo pipefail
# Git Bash on Windows would rewrite "/index.html" into a Windows path.
export MSYS_NO_PATHCONV=1

BUCKET="cad.greendealer.co.nz"
REGION="ap-southeast-6"
DISTRIBUTION="E33B20129EA1HO"

cd "$(dirname "$0")/.."
npm run build

# Hashed assets never change: cache them for a year. Upload them before
# index.html so a new index never points at files that aren't there yet.
aws s3 sync dist/assets "s3://$BUCKET/assets" --region "$REGION" \
  --cache-control "public,max-age=31536000,immutable" --exclude "*.wasm"
aws s3 sync dist/assets "s3://$BUCKET/assets" --region "$REGION" \
  --cache-control "public,max-age=31536000,immutable" --content-type application/wasm \
  --exclude "*" --include "*.wasm"
aws s3 sync dist "s3://$BUCKET" --region "$REGION" --exclude "assets/*" --exclude "index.html" \
  --cache-control "public,max-age=86400"
aws s3 cp dist/index.html "s3://$BUCKET/index.html" --region "$REGION" \
  --cache-control "no-cache" --content-type "text/html; charset=utf-8"

aws cloudfront create-invalidation --distribution-id "$DISTRIBUTION" --paths "/" "/index.html" \
  --query "Invalidation.Id" --output text
echo "Deployed to https://$BUCKET"
