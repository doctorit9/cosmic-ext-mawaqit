# Distribution & Packaging Guide for Mawaqit (مواقيت)

This guide covers how to distribute **Mawaqit** (`cosmic-ext-mawaqit`) across major package managers: **Pacman (Arch/AUR)**, **DNF (Fedora Copr)**, **APT (Debian/Ubuntu/Pop!_OS)**, and **Flatpak**.

---

## 1. Pacman (Arch Linux / AUR)

The standard way to provide packages for `pacman` users is via the **Arch User Repository (AUR)**.

### Submitting to AUR:
1. Register an account at [aur.archlinux.org](https://aur.archlinux.org) and add your SSH public key.
2. Clone the empty AUR repository:
   ```bash
   git clone ssh://aur@aur.archlinux.org/cosmic-ext-mawaqit-git.git
   cd cosmic-ext-mawaqit-git
   ```
3. Copy `packaging/arch/PKGBUILD` into the repository.
4. Generate the `.SRCINFO` metadata file:
   ```bash
   makepkg --printsrcinfo > .SRCINFO
   ```
5. Commit and push:
   ```bash
   git add PKGBUILD .SRCINFO
   git commit -m "Initial release"
   git push origin master
   ```
6. **User installation:** Arch / EndeavourOS users can now install it with:
   ```bash
   yay -S cosmic-ext-mawaqit-git
   # or
   paru -S cosmic-ext-mawaqit-git
   ```

---

## 2. DNF (Fedora / RHEL via Copr)

Fedora provides **Copr** (Cool Other Package Repo), a free automated build service for RPM and `dnf`.

### Publishing on Fedora Copr:
1. Log in to [copr.fedorainfracloud.org](https://copr.fedorainfracloud.org) with your Fedora FAS account.
2. Click **New Project** and name it `cosmic-ext-mawaqit`.
3. Add a new package:
   - Source type: **Custom** or **SCM (Git)** pointing to `https://github.com/doctorit9/cosmic-ext-mawaqit`.
   - Spec file: `packaging/rpm/cosmic-ext-mawaqit.spec`.
4. Trigger a build. Copr will automatically build `.rpm` packages for Fedora versions.
5. **User installation:** Fedora users can now install it with:
   ```bash
   sudo dnf copr enable doctorit9/cosmic-ext-mawaqit
   sudo dnf install cosmic-ext-mawaqit
   ```

---

## 3. APT (Debian, Ubuntu, Pop!_OS)

For Debian/Ubuntu/Pop!_OS users using `apt`:

### Option A: Direct `.deb` Package (Easiest)
1. Install `cargo-deb`:
   ```bash
   cargo install cargo-deb
   ```
2. Build the `.deb` package:
   ```bash
   just deb
   ```
   The `.deb` file will be generated in `target/debian/cosmic-ext-mawaqit_*.deb`.
3. Upload this `.deb` file to your [GitHub Releases](https://github.com/doctorit9/cosmic-ext-mawaqit/releases).
4. Users install with:
   ```bash
   sudo apt install ./cosmic-ext-mawaqit_*.deb
   ```

### Option B: Launchpad PPA (Automatic `apt-get`)
1. Create a PPA on Launchpad (`ppa:doctorit9/cosmic-ext-mawaqit`).
2. Build the source package using `debuild -S` and upload with `dput`.
3. Users install via:
   ```bash
   sudo add-apt-repository ppa:doctorit9/cosmic-ext-mawaqit
   sudo apt update
   sudo apt install cosmic-ext-mawaqit
   ```

---

## 4. Flatpak (Flathub)

A Flatpak manifest is provided in `packaging/flatpak/io.github.doctorit.mawaqit.yml`.

### Important Note for COSMIC Applets:
COSMIC panel applets communicate via Wayland layer-shell protocol and DBus session tokens with `cosmic-panel`. For optimal desktop integration and system panel discovery, native system packages (AUR, Copr, `.deb`) are the recommended format.

To test building locally with `flatpak-builder`:
```bash
flatpak-builder --user --install --force-clean build-dir packaging/flatpak/io.github.doctorit.mawaqit.yml
```
To submit to Flathub, submit a pull request with the manifest to [flathub/flathub](https://github.com/flathub/flathub).
