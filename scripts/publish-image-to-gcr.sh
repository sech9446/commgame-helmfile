#!/bin/bash

set -e

IMAGE="$1"
TAG="$(git rev-parse --short HEAD)"

if [ -z "$IMAGE" ]; then
  echo "image-name is missing."
  echo "hint: $0 <image-name> <tag>"
  exit 1
fi
if [ -z "$TAG" ]; then
  echo "tag of image is missing."
  echo "hint: $0 <image-name> <tag>"
  exit 1
fi

ROOT="$( cd "$( dirname "${BASH_SOURCE[0]}" )/../" >/dev/null 2>&1 && pwd )"

for dir in "$ROOT"/images/*
do
  dir=${dir%*/} 
  name=${dir##*/}
  name=$(echo "$name" | sed 's/^[0-9_\-]*//g')
  DOCKER_BUILDKIT=1 docker build -t "$name:$TAG" "$dir"
  if [ "$IMAGE" == "$name" ]; then 
    break
  fi
done

RESULT=''
for dir in "$ROOT"/images/*
do
  dir=${dir%*/} 
  name=${dir##*/}
  name=$(echo "$name" | sed 's/^[0-9_\-]*//g')
  echo $name
  if [ "$IMAGE" == "$name" ]; then 
    url="gcr.io/${GCP_PROJECT}/${IMAGE}"
    DOCKER_BUILDKIT=1 docker tag "$name:$TAG" "$url:$TAG"
    DOCKER_BUILDKIT=1 docker push "$url:$TAG"
    RESULT="$RESULT
$url:$TAG"
  fi
done

echo "The updated repositories:"
echo "$RESULT"
