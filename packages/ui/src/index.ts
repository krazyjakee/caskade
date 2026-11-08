/** `@caskade/ui`: the shell's themeable components. UI plugins
 * import the app's instance through the import map. */

export {
  Avatar,
  AvatarPicker,
  type AvatarPickerProps,
  type AvatarSize,
  initials,
} from "./Avatar";
export {
  type AnatomyEntry,
  type AnatomyName,
  anatomy,
  anatomyNames,
  anatomyRoots,
  checkAnatomy,
  ownedParts,
} from "./anatomy";
export { type AvatarInfo, avatarInfo, avatars, DEFAULT_AVATAR } from "./avatars";
export { ErrorNotice, type ErrorNoticeProps } from "./ErrorNotice";
export { Button, type ButtonProps } from "./form/Button";
export { Checkbox, type CheckboxProps } from "./form/Checkbox";
export {
  SegmentedControl,
  type SegmentedControlProps,
  type SegmentedOption,
} from "./form/SegmentedControl";
export { Select, type SelectOption, type SelectProps } from "./form/Select";
export { Pill, SettingRow, SettingsSection, StatusDot } from "./form/SettingsSection";
export { Slider, type SliderProps } from "./form/Slider";
export { Switch, type SwitchProps } from "./form/Switch";
export { TextField, type TextFieldProps } from "./form/TextField";
export { artBackground, GameArt } from "./GameArt";
export { type CardGame, GameCard } from "./GameCard";
export { Hero, type HeroGame } from "./Hero";
export {
  ContextMenu,
  type ContextMenuProps,
  contextMenuTarget,
  Menu,
  type MenuAnchor,
  MenuItem,
  type MenuItemProps,
  MenuPopover,
  type MenuProps,
  MenuSection,
  MenuSeparator,
  useContextMenu,
} from "./Menu";
export { Shelf } from "./Shelf";
export { type GameStatus, StatusBadge } from "./StatusBadge";
