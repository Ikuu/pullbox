---

## AppImage

Download `comic-editor-@VERSION@-x86_64.AppImage`, then make it executable and run it. It includes everything it needs.

```sh
chmod +x comic-editor-@VERSION@-x86_64.AppImage
./comic-editor-@VERSION@-x86_64.AppImage
```

## Tarball

Installs Comic Editor for the current user, with a launcher entry. It needs `dav1d`, `unrar` and `avifenc` (from libavif) installed; see the [README](https://github.com/Ikuu/comic-editor#requirements).

```sh
tar xzf comic-editor-@VERSION@-linux-x86_64.tar.gz
cd comic-editor-@VERSION@-linux-x86_64
./install.sh
```

Both run on x86-64 Linux with glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40, Arch and later).
