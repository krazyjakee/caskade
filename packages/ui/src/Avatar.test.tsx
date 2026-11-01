import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Avatar, AvatarPicker, initials } from "./Avatar";
import { avatars, DEFAULT_AVATAR } from "./avatars";

afterEach(cleanup);

describe("the avatar set", () => {
  it("ships at least 24 avatars with stable, unique ids, as bundled SVG", () => {
    expect(avatars.length).toBeGreaterThanOrEqual(24);
    expect(avatars[0]?.id).toBe(DEFAULT_AVATAR);
    expect(new Set(avatars.map((a) => a.id)).size).toBe(avatars.length);
    for (const avatar of avatars) {
      expect(avatar.id).toMatch(/^avatar:[a-z]+$/);
      expect(avatar.url.startsWith("data:image/svg+xml,")).toBe(true);
      expect(decodeURIComponent(avatar.url)).toContain('viewBox="0 0 64 64"');
    }
  });

  it("is the list the core accepts, in the same order", () => {
    // Resolved by path: Vite rewrites `new URL(…, import.meta.url)` into an
    // asset URL under jsdom.
    const here = dirname(fileURLToPath(import.meta.url));
    const source = readFileSync(
      resolve(here, "../../../crates/caskade-core/src/accounts/mod.rs"),
      "utf8",
    );
    const start = source.indexOf("pub const AVATARS");
    const list = source.slice(start, source.indexOf("];", start));
    const core = ["avatar:default", ...[...list.matchAll(/"(avatar:[a-z]+)"/g)].map((m) => m[1])];
    expect(core).toEqual(avatars.map((a) => a.id));
  });
});

describe("Avatar", () => {
  it("shows the avatar's image", () => {
    const { container } = render(<Avatar avatar="avatar:rocket" name="Ada" size="xl" />);
    const root = container.firstElementChild as HTMLElement;
    expect(root.dataset.source).toBe("image");
    expect(root.dataset.size).toBe("xl");
    expect(container.querySelector("img")?.getAttribute("src")).toMatch(/^data:image\/svg/);
  });

  it("shows initials for an avatar this release doesn't ship", () => {
    const { container } = render(<Avatar avatar="avatar:retired" name="Ada Lovelace" />);
    const root = container.firstElementChild as HTMLElement;
    expect(root.dataset.source).toBe("initials");
    expect(container.querySelector("img")).toBeNull();
    expect(root.textContent).toBe("AL");
  });

  it("falls back to initials when the image can't load", () => {
    const { container } = render(<Avatar avatar="avatar:moon" name="kid" />);
    fireEvent.error(container.querySelector("img") as HTMLImageElement);
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toBe("K");
  });

  it("is named only when labelled", () => {
    render(<Avatar avatar="avatar:moon" name="Kid" label="Kid's avatar" />);
    expect(screen.getByRole("img", { name: "Kid's avatar" })).toBeTruthy();
  });

  it("takes up to two initials", () => {
    expect(initials("  ada  ")).toBe("A");
    expect(initials("Ada Byron Lovelace")).toBe("AL");
    expect(initials("")).toBe("?");
  });
});

describe("AvatarPicker", () => {
  it("offers every avatar and marks the chosen one", () => {
    render(<AvatarPicker value="avatar:rocket" onChange={() => {}} />);
    const radios = screen.getAllByRole("radio");
    expect(radios).toHaveLength(avatars.length);
    expect((screen.getByRole("radio", { name: "Rocket" }) as HTMLInputElement).checked).toBe(true);
  });

  it("chooses with a click", () => {
    const onChange = vi.fn();
    render(<AvatarPicker value="avatar:rocket" onChange={onChange} />);
    fireEvent.click(screen.getByRole("radio", { name: "Moon" }));
    expect(onChange).toHaveBeenCalledWith("avatar:moon");
  });

  it("leaves with Escape", () => {
    const onCancel = vi.fn();
    const onChange = vi.fn();
    render(<AvatarPicker value="avatar:rocket" onChange={onChange} onCancel={onCancel} />);
    fireEvent.keyDown(screen.getByRole("radio", { name: "Rocket" }), { key: "Escape" });
    expect(onCancel).toHaveBeenCalledOnce();
    expect(onChange).not.toHaveBeenCalled();
  });
});
