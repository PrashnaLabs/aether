#!/usr/bin/env python3
"""Build and package Aether on the host OS. Requires Python 3.11 or newer."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
ICONS = ROOT / "crates/app/assets/app-icons"
APP_ID = "dev.aether.Aether"


def run(*args, **kwargs):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), check=True, cwd=ROOT, **kwargs)


def copy(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def first_identity(listing):
    for line in listing.splitlines():
        parts = line.split()
        if len(parts) > 1 and parts[0].endswith(")") and len(parts[1]) == 40:
            return parts[1]
    return None


def macos(binary, stage, version, identity):
    app = stage / "Aether.app"
    contents = app / "Contents"
    copy(binary, contents / "MacOS/aether")
    copy(ICONS / "Aether.icns", contents / "Resources/Aether.icns")
    copy(ROOT / "LICENSE", contents / "Resources/LICENSE")
    info = {
        "CFBundleExecutable": "aether",
        "CFBundleIdentifier": APP_ID,
        "CFBundleName": "Aether",
        "CFBundleDisplayName": "Aether",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "CFBundleIconFile": "Aether.icns",
        "NSHumanReadableCopyright": "Copyright © 2026 PrashnaLabs. MIT License.",
        "LSMinimumSystemVersion": os.environ.get("MACOSX_DEPLOYMENT_TARGET", "12.0"),
        "NSHighResolutionCapable": True,
        "NSMicrophoneUsageDescription": "Aether turns your speech into text in the composer. Audio stays on this device.",
        "NSSupportsAutomaticGraphicsSwitching": True,
    }
    with (contents / "Info.plist").open("wb") as file:
        plistlib.dump(info, file)
    run("plutil", "-lint", contents / "Info.plist")
    # An ad-hoc signature is pinned to the build's hash, so each rebuild loses
    # macOS permission grants. Local builds use an installed certificate if any.
    listing = subprocess.run(["security", "find-identity", "-v", "-p", "codesigning"],
                             capture_output=True, text=True).stdout
    signing = ["codesign", "--force", "--entitlements", ROOT / "packaging/macos/Aether.entitlements",
               "--sign", identity or first_identity(listing) or "-"]
    signing += ["--options", "runtime", "--timestamp"] if identity else ["--timestamp=none"]
    run(*signing, app)
    run("codesign", "--verify", "--deep", "--strict", app)
    return app


def linux(binary, stage):
    copy(binary, stage / "usr/bin/aether")
    copy(ROOT / "LICENSE", stage / "usr/share/doc/aether/copyright")
    copy(ROOT / f"packaging/linux/{APP_ID}.desktop",
         stage / f"usr/share/applications/{APP_ID}.desktop")
    for size in (16, 32, 48, 64, 128, 256, 512, 1024):
        copy(ICONS / f"{size}.png",
             stage / f"usr/share/icons/hicolor/{size}x{size}/apps/{APP_ID}.png")
    copy(ICONS / "aether.svg",
         stage / f"usr/share/icons/hicolor/scalable/apps/{APP_ID}.svg")


def debian(stage, artifact, version, arch):
    # Resolve actual linked libraries against the host distribution's packages.
    # The executable lives under usr/bin because dpkg-shlibdeps searches there.
    debian_dir = stage / "debian"
    debian_dir.mkdir()
    (debian_dir / "control").write_text(
        "Source: aether\nSection: devel\nPriority: optional\n"
        "Maintainer: Aether contributors <noreply@localhost>\n"
        "Standards-Version: 4.7.0\n\nPackage: aether\nArchitecture: any\n"
        "Description: Agent workspace\n"
    )
    result = subprocess.run(
        ["dpkg-shlibdeps", "-O", "-eusr/bin/aether"],
        cwd=stage, check=True, text=True, capture_output=True,
    )
    if result.stderr:
        print(result.stderr, end="")
    depends = next(line.split("=", 1)[1] for line in result.stdout.splitlines()
                   if line.startswith("shlibs:Depends="))
    shutil.rmtree(debian_dir)
    control = stage / "DEBIAN/control"
    control.parent.mkdir()
    control.write_text(
        f"Package: aether\nVersion: {version}\nArchitecture: {arch}\n"
        "Section: devel\nPriority: optional\n"
        "Maintainer: Aether contributors <noreply@localhost>\n"
        f"Depends: {depends}\n"
        "Recommends: git, ca-certificates, mesa-vulkan-drivers\n"
        "Homepage: https://aether.prashnalabs.com\n"
        "Description: Coding agents side by side, in one native window\n"
        " Aether runs Claude Code, Codex, OpenCode, Pi, and Antigravity side by\n"
        " side, with terminals, Git worktrees, and a project file explorer.\n"
    )
    run("dpkg-deb", "--root-owner-group", "--build", stage, artifact)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=("app", "dmg", "tar", "deb"),
                        help="Default: dmg on macOS, tar on Linux")
    parser.add_argument("--sign-identity", default=os.environ.get("APPLE_SIGNING_IDENTITY"))
    parser.add_argument("--notary-profile", default=os.environ.get("APPLE_NOTARY_PROFILE"),
                        help="Keychain profile previously created with notarytool store-credentials")
    args = parser.parse_args()
    system = platform.system()
    fmt = args.format or ("dmg" if system == "Darwin" else "tar")
    if system not in ("Darwin", "Linux"):
        parser.error("build on macOS or Linux")
    if fmt not in (("app", "dmg", "tar") if system == "Darwin" else ("tar", "deb")):
        parser.error(f"{fmt} is unsupported on {system}")
    if args.notary_profile and (not args.sign_identity or fmt != "dmg"):
        parser.error("notarization requires --sign-identity and --format dmg")
    if system == "Linux" and (args.sign_identity or args.notary_profile):
        parser.error("Apple signing options require macOS")
    tools = ["cargo"]
    if system == "Darwin":
        tools += ["codesign", "plutil"]
        if fmt == "dmg":
            tools += ["hdiutil"]
        if args.notary_profile:
            tools += ["xcrun"]
    elif fmt == "deb":
        tools += ["dpkg-deb", "dpkg-shlibdeps"]
    for tool in tools:
        if not shutil.which(tool):
            parser.error(f"required tool is missing: {tool}")
    if system == "Darwin":
        os.environ.setdefault("MACOSX_DEPLOYMENT_TARGET", "12.0")
    # whisper.cpp otherwise tunes for this machine's CPU and can crash on lesser ones.
    os.environ.setdefault("GGML_NATIVE", "OFF")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    arch = {"arm64": "aarch64", "AMD64": "x86_64"}.get(platform.machine(), platform.machine())
    if arch not in ("aarch64", "x86_64"):
        parser.error(f"unsupported CPU architecture: {arch}")
    # Explicit target prevents a user's Cargo default target from cross-compiling.
    target = f"{arch}-{'apple-darwin' if system == 'Darwin' else 'unknown-linux-gnu'}"
    result = run("cargo", "build", "--locked", "--release", "-p", "aether", "--target", target,
                 "--message-format=json-render-diagnostics", stdout=subprocess.PIPE, text=True)
    binaries = [Path(message["executable"]) for line in result.stdout.splitlines()
                if (message := json.loads(line)).get("reason") == "compiler-artifact"
                and message.get("executable") and message["target"]["name"] == "aether"]
    if len(binaries) != 1:
        raise RuntimeError("Cargo did not report exactly one Aether executable")
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    stem = f"Aether-{version}-{'macos' if system == 'Darwin' else 'linux'}-{arch}"
    artifact = dist / ("Aether.app" if fmt == "app" else f"{stem}.{'tar.gz' if fmt == 'tar' else fmt}")
    with tempfile.TemporaryDirectory(prefix="aether-build-") as temp:
        stage = Path(temp)
        stage.chmod(0o755)
        if system == "Darwin":
            app = macos(binaries[0], stage, version, args.sign_identity)
        else:
            linux(binaries[0], stage)
        if fmt == "app":
            if artifact.exists():
                shutil.rmtree(artifact)
            shutil.copytree(app, artifact)
        elif fmt == "dmg":
            (stage / "Applications").symlink_to("/Applications")
            # hdiutil sizes -srcfolder by allocated blocks, which APFS compression undercounts
            size = sum(f.lstat().st_size for f in stage.rglob("*") if f.is_file())
            run("hdiutil", "create", "-volname", "Aether", "-srcfolder", stage,
                "-size", f"{size * 5 // 4 // 2**20 + 32}m", "-ov", "-format", "UDZO", artifact)
            if args.sign_identity:
                run("codesign", "--sign", args.sign_identity, "--timestamp", artifact)
            if args.notary_profile:
                run("xcrun", "notarytool", "submit", artifact,
                    "--keychain-profile", args.notary_profile, "--wait")
                run("xcrun", "stapler", "staple", artifact)
                run("xcrun", "stapler", "validate", artifact)
        elif fmt == "tar":
            with tarfile.open(artifact, "w:gz") as archive:
                for child in sorted(stage.iterdir()):
                    archive.add(child, arcname=f"{stem}/{child.name}")
        else:
            debian(stage, artifact, version, {"x86_64": "amd64", "aarch64": "arm64"}[arch])
    if artifact.is_file():
        with artifact.open("rb") as file:
            digest = hashlib.file_digest(file, "sha256").hexdigest()
        artifact.with_name(artifact.name + ".sha256").write_text(f"{digest}  {artifact.name}\n")
    print(f"Built {artifact}")


if __name__ == "__main__":
    main()
