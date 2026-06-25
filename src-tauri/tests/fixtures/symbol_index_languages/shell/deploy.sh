#!/usr/bin/env bash

source "./lib/common.sh"
. "./env.sh"

function deploy_app() {
  echo "deploy"
}

rollback_app() {
  echo "rollback"
}

cleanup-trap() {
  echo "cleanup"
}
