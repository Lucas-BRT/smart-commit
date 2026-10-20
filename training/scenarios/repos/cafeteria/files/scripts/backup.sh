#!/bin/sh
# Copia o banco do estoque para backups/, com a data no nome.
set -eu
mkdir -p backups
cp estoque.sqlite "backups/estoque-$(date +%F).sqlite"
