---

## AppImage

Download `pullbox-@VERSION@-x86_64.AppImage`, then make it executable and run it. It includes everything it needs.

```sh
chmod +x pullbox-@VERSION@-x86_64.AppImage
./pullbox-@VERSION@-x86_64.AppImage
```

## Tarball

Installs Pullbox for the current user, with a launcher entry. It needs `dav1d`, `unrar` and `avifenc` (from libavif) installed; see the [README](https://github.com/Ikuu/pullbox#requirements).

```sh
tar xzf pullbox-@VERSION@-linux-x86_64.tar.gz
cd pullbox-@VERSION@-linux-x86_64
./install.sh
```

Both run on x86-64 Linux with glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40, Arch and later).
