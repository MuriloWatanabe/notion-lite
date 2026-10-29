#!/usr/bin/env bash
# Lança uma nova versão do Notion Lite.
#
#   scripts/release.sh 0.2.0
#
# 1. Atualiza a versão em Cargo.toml, Cargo.lock, aur/notion-lite/PKGBUILD e .SRCINFO
# 2. Faz commit, cria a tag v0.2.0 e envia para o GitHub
#    → o GitHub Actions compila e publica o pacote em Releases
# 3. Se AUR_DIR apontar para o clone do AUR (ex.: ~/aur/notion-lite), também publica lá
#
# Variáveis: AUR_DIR=<clone do AUR>  NO_PUSH=1 (só prepara, não envia nada)
set -euo pipefail

v=${1:-}
if [[ ! $v =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "uso: scripts/release.sh X.Y.Z   (ex.: scripts/release.sh 0.2.0)" >&2
  exit 1
fi

cd "$(git rev-parse --show-toplevel)"
if [[ -n $(git status --porcelain) ]]; then
  echo "Há mudanças não commitadas. Faça commit (ou git stash) antes de lançar." >&2
  exit 1
fi
if git rev-parse -q --verify "refs/tags/v$v" >/dev/null; then
  echo "A tag v$v já existe." >&2
  exit 1
fi

echo "==> Versão $v"
# Cargo.toml: primeira linha "version = ..." (a do [package])
sed -i "0,/^version = \".*\"/s//version = \"$v\"/" Cargo.toml
# Cargo.lock: versão do próprio pacote
sed -i "/^name = \"notion-lite\"$/{n;s/^version = \".*\"/version = \"$v\"/}" Cargo.lock
# PKGBUILD estável
sed -i "s/^pkgver=.*/pkgver=$v/; s/^pkgrel=.*/pkgrel=1/" aur/notion-lite/PKGBUILD
# .SRCINFO
if command -v makepkg >/dev/null; then
  (cd aur/notion-lite && makepkg --printsrcinfo > .SRCINFO)
else
  sed -i -E "s/(pkgver = ).*/\1$v/; s/(pkgrel = ).*/\11/; s/notion-lite-[0-9.]+\.tar\.gz/notion-lite-$v.tar.gz/; s#/v[0-9.]+\.tar\.gz#/v$v.tar.gz#" aur/notion-lite/.SRCINFO
fi

git add Cargo.toml Cargo.lock aur/notion-lite/PKGBUILD aur/notion-lite/.SRCINFO
if git diff --cached --quiet; then
  echo "==> Arquivos já estavam na versão $v"
else
  git commit -q -m "release v$v"
fi
git tag -a "v$v" -m "Notion Lite $v"

if [[ -n ${NO_PUSH:-} ]]; then
  echo "==> NO_PUSH: commit e tag criados localmente, nada enviado."
  exit 0
fi

git push origin HEAD
git push origin "v$v"
repo=$(git remote get-url origin | sed -E 's#(git@github.com:|https://github.com/)##; s#\.git$##')
echo "==> Enviado. O pacote aparece em alguns minutos em https://github.com/$repo/releases/tag/v$v"

# Publicação no AUR (quando a conta existir)
if [[ -n ${AUR_DIR:-} ]]; then
  echo "==> Atualizando o AUR em $AUR_DIR"
  cp aur/notion-lite/PKGBUILD "$AUR_DIR/"
  cd "$AUR_DIR"
  git pull -q --rebase || true
  updpkgsums                         # baixa o tarball da tag e grava o sha256 real
  makepkg --printsrcinfo > .SRCINFO
  git add PKGBUILD .SRCINFO
  git commit -q -m "notion-lite $v"
  git push
  echo "==> AUR atualizado: yay -Syu notion-lite"
fi
