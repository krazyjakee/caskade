use crate::{executable, images, write};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureOs {
    Linux,
    Windows,
    Macos,
}

/// Owns an isolated HostEnv-compatible scenario. Drop removes all generated
/// bytes. Detection files are markers, not real emulator/core/BIOS software.
pub struct MachineFixture {
    temp: tempfile::TempDir,
    pub os: FixtureOs,
    pub home: PathBuf,
}

impl MachineFixture {
    pub fn new(os: FixtureOs) -> io::Result<Self> {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join("root"))?;
        fs::create_dir(temp.path().join("registry"))?;
        let (key, home, env) = match os {
            FixtureOs::Linux => (
                "linux",
                "home/fixture",
                "PATH='/usr/local/bin:/usr/bin'\nXDG_CONFIG_HOME='/home/fixture/.config'\nXDG_DATA_HOME='/home/fixture/.local/share'\n",
            ),
            FixtureOs::Macos => (
                "macos",
                "Users/fixture",
                "PATH='/opt/homebrew/bin:/usr/local/bin:/usr/bin'\n",
            ),
            FixtureOs::Windows => (
                "windows",
                "drives/c/Users/fixture",
                "PATH='C:\\Tools;C:\\ProgramData\\chocolatey\\bin'\nPATHEXT='.EXE;.CMD'\n",
            ),
        };
        write(
            &temp.path().join("machine.toml"),
            format!("schema_version=1\nos='{key}'\nnow_unix_secs=1700000000\n[env]\n{env}"),
        )?;
        let home = temp.path().join("root").join(home);
        fs::create_dir_all(&home)?;
        let fixture = Self { temp, os, home };
        match os {
            FixtureOs::Linux => fixture.linux()?,
            FixtureOs::Windows => fixture.windows()?,
            FixtureOs::Macos => fixture.macos()?,
        }
        Ok(fixture)
    }
    pub fn path(&self) -> &Path {
        self.temp.path()
    }
    pub fn root(&self) -> PathBuf {
        self.path().join("root")
    }
    pub fn flatpak_list(&self) -> io::Result<String> {
        fs::read_to_string(self.path().join("recorded/flatpak-list.tsv"))
    }
    fn marker(&self, relative: &str) -> io::Result<()> {
        executable(
            &self.root().join(relative),
            b"Caskade original synthetic emulator marker; static probes only\n",
        )
    }
    fn linux(&self) -> io::Result<()> {
        self.marker("usr/bin/retroarch")?;
        self.marker("usr/local/bin/mgba")?;
        self.marker("opt/emulators/DuckStation/duckstation-qt")?;
        for (name, id, relative) in [
            (
                "PCSX2",
                "net.pcsx2.PCSX2",
                "Applications/PCSX2-v2.0.AppImage",
            ),
            (
                "Dolphin",
                "org.DolphinEmu.dolphin-emu",
                ".local/bin/Dolphin-x86_64.AppImage",
            ),
        ] {
            images::appimage(&self.home.join(relative), name, id)?;
        }
        let install = self.home.join(".local/share/flatpak");
        let id = "org.libretro.RetroArch";
        let active = install.join(format!("app/{id}/current/active"));
        write(
            &active.join("metadata"),
            format!("[Application]\nname={id}\n[Context]\nfilesystems=home;\n"),
        )?;
        write(
            &active.join("files/bin/retroarch"),
            b"synthetic deployment marker",
        )?;
        // Runtime and invalid app records exercise the same negative cases as
        // the production file reader; neither is an installed emulator.
        write(
            &install.join("runtime/org.freedesktop.Platform/current/active/metadata"),
            b"[Runtime]\nname=org.freedesktop.Platform\n",
        )?;
        write(
            &install.join("app/org.example.Incomplete/current/active/metadata"),
            b"[Application]\nname=org.example.Incomplete\n",
        )?;
        write(
            &self
                .home
                .join(".var/app/org.libretro.RetroArch/config/retroarch/cores/snes9x_libretro.so"),
            [],
        )?;
        write(&self.home.join(".var/app/org.libretro.RetroArch/config/retroarch/cores/mednafen_psx_hw_libretro.so"), [])?;
        write(
            &self.home.join(".config/retroarch/cores/snes9x_libretro.so"),
            [],
        )?;
        write(&self.home.join(".local/share/applications/retroarch.desktop"), b"[Desktop Entry]\nType=Application\nName=RetroArch\nExec=retroarch %f\nCategories=Game;Emulator;\n")?;
        write(
            &self.path().join("recorded/flatpak-list.tsv"),
            b"RetroArch\torg.libretro.RetroArch\t1.20.0\tstable\tuser\n",
        )?;
        self.marker("home/fixture/Emulation/tools/RetroArch/retroarch")?;
        images::appimage(
            &self.home.join("Applications/RetroDECK.AppImage"),
            "RetroDECK",
            "net.retrodeck.retrodeck",
        )?;
        write(
            &self.home.join(".config/EmuDeck/settings.sh"),
            b"# Synthetic format marker; never sourced\nromsPath='/home/fixture/Emulation/roms'\n",
        )?;
        write(
            &self
                .home
                .join(".var/app/net.retrodeck.retrodeck/config/retrodeck/retrodeck.cfg"),
            b"roms_folder=/home/fixture/retrodeck/roms\n",
        )?;
        fs::create_dir_all(self.home.join("Emulation/roms"))?;
        fs::create_dir_all(self.home.join("retrodeck/roms"))
    }
    fn windows(&self) -> io::Result<()> {
        for path in [
            "drives/c/Program Files/RetroArch/retroarch.exe",
            "drives/c/Program Files (x86)/PCSX2/pcsx2-qt.exe",
            "drives/c/Users/fixture/AppData/Roaming/RetroArch/retroarch.exe",
            "drives/c/Users/fixture/AppData/Local/DuckStation/duckstation-qt-x64-ReleaseLTCG.exe",
            "drives/c/Users/fixture/scoop/apps/retroarch/current/retroarch.exe",
            "drives/c/ProgramData/chocolatey/bin/retroarch.exe",
            "drives/c/ProgramData/chocolatey/lib/retroarch/tools/retroarch.exe",
            "drives/c/Tools/retroarch.exe",
            "drives/d/Portable Emulators/Dolphin/Dolphin.exe",
        ] {
            self.marker(path)?;
        }
        write(
            &self
                .home
                .join("AppData/Roaming/RetroArch/cores/snes9x_libretro.dll"),
            [],
        )?;
        write(
            &self
                .root()
                .join("drives/c/Program Files/RetroArch/cores/snes9x_libretro.dll"),
            [],
        )
    }
    fn macos(&self) -> io::Result<()> {
        self.marker("opt/homebrew/bin/retroarch")?;
        self.marker("usr/local/bin/mgba")?;
        for (relative, name, id) in [
            (
                "Applications/RetroArch.app",
                "RetroArch",
                "com.libretro.RetroArch",
            ),
            (
                "Users/fixture/Applications/Dolphin.app",
                "Dolphin",
                "org.dolphin-emu.dolphin",
            ),
        ] {
            let bundle = self.root().join(relative);
            write(
                &bundle.join("Contents/Info.plist"),
                format!(
                    "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleName</key><string>{name}</string><key>CFBundleExecutable</key><string>{name}</string><key>CFBundleShortVersionString</key><string>1.0.0</string></dict></plist>"
                ),
            )?;
            executable(
                &bundle.join("Contents/MacOS").join(name),
                b"synthetic static probe marker",
            )?;
        }
        write(
            &self
                .home
                .join("Library/Application Support/RetroArch/cores/snes9x_libretro.dylib"),
            [],
        )
    }
}
