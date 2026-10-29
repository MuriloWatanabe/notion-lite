# Publicando no AUR

O AUR usa **chave SSH**, não senha nem token. Faça os passos 1 e 2 uma vez só.

## 1. Chave SSH

```bash
ssh-keygen -t ed25519 -C "mwatanabe.pro@gmail.com" -f ~/.ssh/aur
cat ~/.ssh/aur.pub        # copie esta linha
```

Adicione no `~/.ssh/config`:

```
Host aur.archlinux.org
  IdentityFile ~/.ssh/aur
  User aur
```

## 2. Conta no AUR

Crie a conta em https://aur.archlinux.org/register e cole a chave pública no campo **SSH Public Key**. Para mudar depois, vá em *My Account*.

Teste a conexão:

```bash
ssh aur@aur.archlinux.org help
```

## 3. Versão estável (`notion-lite`)

Publique o código no GitHub e crie a tag:

```bash
git tag v0.1.0 && git push origin v0.1.0
```

Depois:

```bash
git clone ssh://aur@aur.archlinux.org/notion-lite.git aur-notion-lite
cp aur/notion-lite/PKGBUILD aur-notion-lite/
cd aur-notion-lite
updpkgsums                          # troca o SKIP pelo sha256 real do tarball da tag
makepkg -si                         # testa: compila, instala e você abre o app
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO
git commit -m "notion-lite 0.1.0"
git push
```

> Se o `git clone` avisar que o repositório está vazio, é normal: o nome está livre e é seu ao dar o primeiro push. Se já existir outro pacote com esse nome, troque `pkgname` no PKGBUILD.

## 4. Versão de desenvolvimento (`notion-lite-git`)

Mesmo processo, sem precisar de tag nem checksum:

```bash
git clone ssh://aur@aur.archlinux.org/notion-lite-git.git aur-notion-lite-git
cp aur/notion-lite-git/PKGBUILD aur-notion-lite-git/
cd aur-notion-lite-git
makepkg -si
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO && git commit -m "Initial commit" && git push
```

## 5. Novas versões

1. Suba a versão em `Cargo.toml` e em `pkgver` no PKGBUILD, e volte `pkgrel` para 1.
2. Rode `git tag vX.Y.Z && git push origin vX.Y.Z`.
3. No repositório do AUR: `updpkgsums && makepkg --printsrcinfo > .SRCINFO && git commit -am "notion-lite X.Y.Z" && git push`.

Depois de publicado, qualquer pessoa instala com `yay -S notion-lite`.
