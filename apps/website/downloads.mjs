// Select actual installer assets, never updater archives or signatures.
export function downloadLinks(release) {
  if (release.draft || release.prerelease || !release.tag_name) {
    throw new Error("Downloads require a published stable release");
  }
  const suffixes = {
    linuxAppImageUrl: "_amd64.AppImage",
    linuxDebUrl: "_amd64.deb",
    windowsSetupUrl: "_x64-setup.exe",
    macArmUrl: "_aarch64.dmg",
    macIntelUrl: "_x64.dmg",
  };
  return Object.fromEntries(
    Object.entries(suffixes).map(([key, suffix]) => {
      const matches = release.assets.filter((asset) => asset.name.endsWith(suffix));
      if (matches.length !== 1) {
        throw new Error(`Expected one download ending in ${suffix}, found ${matches.length}`);
      }
      const asset = matches[0];
      const expected = `https://github.com/krazyjakee/caskade/releases/download/${encodeURIComponent(release.tag_name)}/${encodeURIComponent(asset.name)}`;
      if (asset.browser_download_url !== expected) {
        throw new Error(`Unexpected download URL for ${asset.name}`);
      }
      return [key, asset.browser_download_url];
    }),
  );
}
