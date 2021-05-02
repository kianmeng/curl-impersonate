use crate::app::clipboard::CopyType;
use crate::app::command::Command;
use crate::app::keys::{KeyBinding, KEY_BINDINGS};
use crate::app::mode::Mode;
use crate::app::prompt::{OutputType, Prompt, COMMAND_PREFIX, SEARCH_PREFIX};
use crate::app::splash::SplashScreen;
use crate::app::state::State;
use crate::app::tab::Tab;
use crate::args::Args;
use crate::gpg::context::GpgContext;
use crate::gpg::key::{GpgKey, KeyDetail, KeyType};
use crate::widget::list::StatefulList;
use crate::widget::row::ScrollDirection;
use crate::widget::style::Color as WidgetColor;
use crate::widget::table::{StatefulTable, TableState};
use anyhow::{anyhow, Error as AnyhowError, Result};
use colorsys::Rgb;
use copypasta_ext::prelude::ClipboardProvider;
use copypasta_ext::x11_fork::ClipboardContext;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command as OsCommand;
use std::str;
use std::str::FromStr;
use tui::style::Color;

/// Max duration of prompt messages.
const MESSAGE_DURATION: u128 = 1750;

/// Main application.
///
/// It is responsible for running the commands
/// for changing the state of the interface.
pub struct App<'a> {
	/// Application state.
	pub state: State,
	/// Application mode.
	pub mode: Mode,
	/// Prompt manager.
	pub prompt: Prompt,
	/// Current tab.
	pub tab: Tab,
	/// Content of the options menu.
	pub options: StatefulList<Command>,
	/// Splash screen of the application.
	pub splash_screen: SplashScreen,
	/// Content of the key bindings list.
	pub key_bindings: StatefulList<KeyBinding<'a>>,
	/// Public/secret keys.
	pub keys: HashMap<KeyType, Vec<GpgKey>>,
	/// Table of public/secret keys.
	pub keys_table: StatefulTable<GpgKey>,
	/// States of the keys table.
	pub keys_table_states: HashMap<KeyType, TableState>,
	/// Level of detail to show for keys table.
	pub keys_table_detail: KeyDetail,
	/// Bottom margin value of the keys table.
	pub keys_table_margin: u16,
	/// Clipboard context.
	pub clipboard: Option<ClipboardContext>,
	/// GPGME context.
	pub gpgme: &'a mut GpgContext,
}

impl<'a> App<'a> {
	/// Constructs a new instance of `App`.
	pub fn new(gpgme: &'a mut GpgContext, args: &'a Args) -> Result<Self> {
		let keys = gpgme.get_all_keys()?;
		let keys_table = StatefulTable::with_items(
			keys.get(&KeyType::Public)
				.expect("failed to get public keys")
				.to_vec(),
		);
		Ok(Self {
			state: State::from(args),
			mode: Mode::Normal,
			prompt: Prompt::default(),
			tab: Tab::Keys(KeyType::Public),
			options: StatefulList::with_items(Vec::new()),
			splash_screen: SplashScreen::new("splash.jpg", 12)?,
			key_bindings: StatefulList::with_items(KEY_BINDINGS.to_vec()),
			keys,
			keys_table,
			keys_table_states: HashMap::new(),
			keys_table_detail: KeyDetail::Minimum,
			keys_table_margin: 1,
			clipboard: match ClipboardContext::new() {
				Ok(clipboard) => Some(clipboard),
				Err(e) => {
					println!("failed to initialize clipboard: {:?}", e);
					None
				}
			},
			gpgme,
		})
	}

	/// Resets the application state.
	pub fn refresh(&mut self) -> Result<()> {
		self.state.refresh();
		self.mode = Mode::Normal;
		self.prompt.clear();
		self.options.state.select(Some(0));
		self.keys = self.gpgme.get_all_keys()?;
		self.keys_table_states.clear();
		self.keys_table_detail = KeyDetail::Minimum;
		self.keys_table_margin = 1;
		match self.tab {
			Tab::Keys(key_type) => {
				self.keys_table = StatefulTable::with_items(
					self.keys
						.get(&key_type)
						.unwrap_or_else(|| {
							panic!("failed to get {} keys", key_type)
						})
						.to_vec(),
				)
			}
			Tab::Help => {}
		};
		Ok(())
	}

	/// Handles the tick event of the application.
	///
	/// It is currently used to flush the prompt messages.
	pub fn tick(&mut self) {
		if let Some(clock) = self.prompt.clock {
			if clock.elapsed().as_millis() > MESSAGE_DURATION
				&& self.prompt.command.is_none()
			{
				self.prompt.clear()
			}
		}
	}

	/// Runs the given command which is used to specify
	/// the widget to render or action to perform.
	pub fn run_command(&mut self, command: Command) -> Result<()> {
		let mut show_options = false;
		if let Command::Confirm(ref cmd) = command {
			self.prompt.set_command(*cmd.clone())
		} else if self.prompt.command.is_some() {
			self.prompt.clear();
		}
		match command {
			Command::ShowHelp => {
				self.tab = Tab::Help;
				if self.key_bindings.state.selected().is_none() {
					self.key_bindings.state.select(Some(0));
				}
			}
			Command::ShowOutput(output_type, message) => {
				self.prompt.set_output((output_type, message))
			}
			Command::ShowOptions => {
				let prev_selection = self.options.state.selected();
				let prev_item_count = self.options.items.len();
				self.options = StatefulList::with_items(match self.tab {
					Tab::Keys(key_type) => {
						let selected_key = &self
							.keys_table
							.selected()
							.expect("invalid selection");
						vec![
							Command::None,
							Command::ShowHelp,
							Command::Refresh,
							Command::RefreshKeys,
							Command::Set(
								String::from("prompt"),
								String::from(":import "),
							),
							Command::Set(
								String::from("prompt"),
								String::from(":receive "),
							),
							Command::ExportKeys(
								key_type,
								vec![selected_key.get_id()],
							),
							Command::ExportKeys(key_type, Vec::new()),
							Command::Confirm(Box::new(Command::DeleteKey(
								key_type,
								selected_key.get_id(),
							))),
							Command::Confirm(Box::new(Command::SendKey(
								selected_key.get_id(),
							))),
							Command::EditKey(selected_key.get_id()),
							Command::SignKey(selected_key.get_id()),
							Command::GenerateKey,
							Command::Set(
								String::from("armor"),
								(!self.gpgme.config.armor).to_string(),
							),
							Command::Copy(CopyType::Key),
							Command::Copy(CopyType::KeyId),
							Command::Copy(CopyType::KeyFingerprint),
							Command::Copy(CopyType::KeyUserId),
							Command::Copy(CopyType::TableRow(1)),
							Command::Copy(CopyType::TableRow(2)),
							Command::Paste,
							Command::ToggleDetail(false),
							Command::ToggleDetail(true),
							Command::Set(
								String::from("margin"),
								String::from(if self.keys_table_margin == 1 {
									"0"
								} else {
									"1"
								}),
							),
							Command::Set(
								String::from("minimized"),
								(!self.state.minimized).to_string(),
							),
							Command::Set(
								String::from("colored"),
								(!self.state.colored).to_string(),
							),
							if self.mode == Mode::Visual {
								Command::SwitchMode(Mode::Normal)
							} else {
								Command::SwitchMode(Mode::Visual)
							},
							Command::Quit,
						]
					}
					Tab::Help => {
						vec![
							Command::None,
							Command::ListKeys(KeyType::Public),
							Command::ListKeys(KeyType::Secret),
							if self.mode == Mode::Visual {
								Command::SwitchMode(Mode::Normal)
							} else {
								Command::SwitchMode(Mode::Visual)
							},
							Command::Refresh,
							Command::Quit,
						]
					}
				});
				if prev_item_count == 0
					|| self.options.items.len() == prev_item_count
				{
					self.options.state.select(prev_selection.or(Some(0)));
				} else {
					self.options.state.select(Some(0));
				}
				show_options = true;
			}
			Command::ListKeys(key_type) => {
				if let Tab::Keys(previous_key_type) = self.tab {
					self.keys_table_states.insert(
						previous_key_type,
						self.keys_table.state.clone(),
					);
					self.keys.insert(
						previous_key_type,
						self.keys_table.default_items.clone(),
					);
				}
				self.keys_table = StatefulTable::with_items(
					self.keys
						.get(&key_type)
						.unwrap_or_else(|| {
							panic!("failed to get {} keys", key_type)
						})
						.to_vec(),
				);
				if let Some(state) = self.keys_table_states.get(&key_type) {
					self.keys_table.state = state.clone();
				}
				self.tab = Tab::Keys(key_type);
			}
			Command::ImportKeys(keys, false) => {
				if keys.is_empty() {
					self.prompt.set_output((
						OutputType::Failure,
						String::from("no files given"),
					))
				} else {
					match self.gpgme.import_keys(keys) {
						Ok(key_count) => {
							self.refresh()?;
							self.prompt.set_output((
								OutputType::Success,
								format!("{} keys imported", key_count),
							))
						}
						Err(e) => self.prompt.set_output((
							OutputType::Failure,
							format!("import error: {}", e),
						)),
					}
				}
			}
			Command::ExportKeys(key_type, ref patterns) => {
				self.prompt.set_output(
					match self
						.gpgme
						.export_keys(key_type, Some(patterns.to_vec()))
					{
						Ok(path) => {
							(OutputType::Success, format!("export: {}", path))
						}
						Err(e) => (
							OutputType::Failure,
							format!("export error: {}", e),
						),
					},
				);
			}
			Command::DeleteKey(key_type, ref key_id) => {
				match self.gpgme.delete_key(key_type, key_id.to_string()) {
					Ok(_) => {
						self.refresh()?;
					}
					Err(e) => self.prompt.set_output((
						OutputType::Failure,
						format!("delete error: {}", e),
					)),
				}
			}
			Command::SendKey(key_id) => {
				self.prompt.set_output(match self.gpgme.send_key(key_id) {
					Ok(key_id) => (
						OutputType::Success,
						format!("key sent to the keyserver: 0x{}", key_id),
					),
					Err(e) => {
						(OutputType::Failure, format!("send error: {}", e))
					}
				});
			}
			Command::GenerateKey
			| Command::RefreshKeys
			| Command::EditKey(_)
			| Command::SignKey(_)
			| Command::ImportKeys(_, true) => {
				let mut os_command = OsCommand::new("gpg");
				let os_command = match command {
					Command::EditKey(key) => {
						os_command.arg("--edit-key").arg(&key)
					}
					Command::SignKey(key) => {
						if let Some(default_key) =
							&self.gpgme.config.default_key
						{
							os_command.arg("--default-key").arg(default_key);
						}
						os_command.arg("--sign-key").arg(&key)
					}
					Command::ImportKeys(keys, _) => {
						os_command.arg("--receive-keys").args(&keys)
					}
					Command::RefreshKeys => os_command.arg("--refresh-keys"),
					_ => os_command.arg("--full-gen-key"),
				};
				match os_command.spawn() {
					Ok(mut child) => {
						child.wait()?;
						self.refresh()?;
					}
					Err(e) => self.prompt.set_output((
						OutputType::Failure,
						format!("execution error: {}", e),
					)),
				}
			}
			Command::ToggleDetail(true) => {
				self.keys_table_detail.increase();
				for key in self.keys_table.items.iter_mut() {
					key.detail = self.keys_table_detail;
				}
				for key in self.keys_table.default_items.iter_mut() {
					key.detail = self.keys_table_detail;
				}
			}
			Command::ToggleDetail(false) => {
				if let Some(index) = self.keys_table.state.tui.selected() {
					if let Some(key) = self.keys_table.items.get_mut(index) {
						key.detail.increase()
					}
					if self.keys_table.items.len()
						== self.keys_table.default_items.len()
					{
						if let Some(key) =
							self.keys_table.default_items.get_mut(index)
						{
							key.detail.increase()
						}
					}
				}
			}
			Command::Scroll(direction, false) => match direction {
				ScrollDirection::Down(_) => {
					if self.state.show_options {
						self.options.next();
						show_options = true;
					} else if Tab::Help == self.tab {
						self.key_bindings.next();
					} else {
						self.keys_table.next();
					}
				}
				ScrollDirection::Up(_) => {
					if self.state.show_options {
						self.options.previous();
						show_options = true;
					} else if Tab::Help == self.tab {
						self.key_bindings.previous();
					} else {
						self.keys_table.previous();
					}
				}
				ScrollDirection::Top => {
					if self.state.show_options {
						self.options.state.select(Some(0));
						show_options = true;
					} else if Tab::Help == self.tab {
						self.key_bindings.state.select(Some(0));
					} else {
						self.keys_table.state.tui.select(Some(0));
					}
				}
				ScrollDirection::Bottom => {
					if self.state.show_options {
						self.options.state.select(Some(
							self.options
								.items
								.len()
								.checked_sub(1)
								.unwrap_or_default(),
						));
						show_options = true;
					} else if Tab::Help == self.tab {
						self.key_bindings
							.state
							.select(Some(KEY_BINDINGS.len() - 1));
					} else {
						self.keys_table.state.tui.select(Some(
							self.keys_table
								.items
								.len()
								.checked_sub(1)
								.unwrap_or_default(),
						));
					}
				}
				_ => {}
			},
			Command::Scroll(direction, true) => {
				self.keys_table.scroll_row(direction);
			}
			Command::Set(option, value) => {
				if option == *"prompt"
					&& (value.starts_with(COMMAND_PREFIX)
						| value.starts_with(SEARCH_PREFIX))
				{
					self.prompt.clear();
					self.prompt.text = value;
				} else {
					self.prompt.set_output(match option.as_str() {
						"output" => {
							let path = Path::new(&value);
							if path.exists() {
								self.gpgme.config.output_dir =
									path.to_path_buf();
								(
									OutputType::Success,
									format!(
										"output directory: {:?}",
										self.gpgme.config.output_dir
									),
								)
							} else {
								(
									OutputType::Failure,
									String::from("path does not exist"),
								)
							}
						}
						"mode" => {
							if let Ok(mode) = Mode::from_str(&value) {
								self.mode = mode;
								(
									OutputType::Success,
									format!(
										"mode: {}",
										format!("{:?}", mode).to_lowercase()
									),
								)
							} else {
								(
									OutputType::Failure,
									String::from("invalid mode"),
								)
							}
						}
						"armor" => {
							if let Ok(value) = FromStr::from_str(&value) {
								self.gpgme.config.armor = value;
								self.gpgme.apply_config();
								(
									OutputType::Success,
									format!("armor: {}", value),
								)
							} else {
								(
									OutputType::Failure,
									String::from(
										"usage: set armor <true/false>",
									),
								)
							}
						}
						"minimized" => {
							self.state.minimize_threshold = 0;
							self.state.minimized =
								FromStr::from_str(&value).unwrap_or_default();
							(
								OutputType::Success,
								format!("minimized: {}", self.state.minimized),
							)
						}
						"minimize" => {
							self.state.minimize_threshold =
								value.parse().unwrap_or_default();
							(
								OutputType::Success,
								format!(
									"minimize threshold: {}",
									self.state.minimize_threshold
								),
							)
						}
						"detail" => {
							if let Ok(detail_level) =
								KeyDetail::from_str(&value)
							{
								if let Some(index) =
									self.keys_table.state.tui.selected()
								{
									if let Some(key) =
										self.keys_table.items.get_mut(index)
									{
										key.detail = detail_level;
									}
									if self.keys_table.items.len()
										== self.keys_table.default_items.len()
									{
										if let Some(key) = self
											.keys_table
											.default_items
											.get_mut(index)
										{
											key.detail = detail_level;
										}
									}
								}
								(
									OutputType::Success,
									format!("detail: {}", detail_level),
								)
							} else {
								(
									OutputType::Failure,
									String::from("usage: set detail <level>"),
								)
							}
						}
						"margin" => {
							self.keys_table_margin =
								value.parse().unwrap_or_default();
							(
								OutputType::Success,
								format!(
									"table margin: {}",
									self.keys_table_margin
								),
							)
						}
						"colored" => match value.parse() {
							Ok(colored) => {
								self.state.colored = colored;
								(
									OutputType::Success,
									format!("colored: {}", self.state.colored),
								)
							}
							Err(_) => (
								OutputType::Failure,
								String::from("usage: set colored <true/false>"),
							),
						},
						"color" => {
							self.state.color =
								WidgetColor::from(value.as_ref()).get();
							(
								OutputType::Success,
								format!(
									"color: {}",
									match self.state.color {
										Color::Rgb(r, g, b) =>
											Rgb::from((r, g, b)).to_hex_string(),
										_ => format!("{:?}", self.state.color)
											.to_lowercase(),
									}
								),
							)
						}
						_ => (
							OutputType::Failure,
							if !option.is_empty() {
								format!("unknown option: {}", option)
							} else {
								String::from("usage: set <option> <value>")
							},
						),
					})
				}
			}
			Command::Get(option) => {
				self.prompt.set_output(match option.as_str() {
					"output" => (
						OutputType::Success,
						format!(
							"output directory: {:?}",
							self.gpgme.config.output_dir.as_os_str()
						),
					),
					"mode" => (
						OutputType::Success,
						format!(
							"mode: {}",
							format!("{:?}", self.mode).to_lowercase()
						),
					),
					"armor" => (
						OutputType::Success,
						format!("armor: {}", self.gpgme.config.armor),
					),
					"minimized" => (
						OutputType::Success,
						format!("minimized: {}", self.state.minimized),
					),
					"minimize" => (
						OutputType::Success,
						format!(
							"minimize threshold: {}",
							self.state.minimize_threshold
						),
					),
					"detail" => {
						if let Some(index) =
							self.keys_table.state.tui.selected()
						{
							if let Some(key) = self.keys_table.items.get(index)
							{
								(
									OutputType::Success,
									format!("detail: {}", key.detail),
								)
							} else {
								(
									OutputType::Failure,
									String::from("invalid selection"),
								)
							}
						} else {
							(
								OutputType::Failure,
								String::from("unknown selection"),
							)
						}
					}
					"margin" => (
						OutputType::Success,
						format!("table margin: {}", self.keys_table_margin),
					),
					"colored" => (
						OutputType::Success,
						format!("colored: {}", self.state.colored),
					),
					"color" => (
						OutputType::Success,
						format!(
							"color: {}",
							match self.state.color {
								Color::Rgb(r, g, b) =>
									Rgb::from((r, g, b)).to_hex_string(),
								_ => format!("{:?}", self.state.color)
									.to_lowercase(),
							}
						),
					),
					_ => (
						OutputType::Failure,
						if !option.is_empty() {
							format!("unknown option: {}", option)
						} else {
							String::from("usage: get <option>")
						},
					),
				})
			}
			Command::SwitchMode(mode) => {
				if !(mode == Mode::Copy && self.keys_table.items.is_empty()) {
					self.mode = mode;
					self.prompt
						.set_output((OutputType::Action, mode.to_string()))
				}
			}
			Command::Copy(copy_type) => {
				let selected_key =
					&self.keys_table.selected().expect("invalid selection");
				let content = match copy_type {
					CopyType::TableRow(1) => Ok(selected_key
						.get_subkey_info(self.state.minimized)
						.join("\n")),
					CopyType::TableRow(2) => Ok(selected_key
						.get_user_info(self.state.minimized)
						.join("\n")),
					CopyType::TableRow(_) => Err(anyhow!("invalid row number")),
					CopyType::Key => {
						match self.gpgme.get_exported_keys(
							match self.tab {
								Tab::Keys(key_type) => key_type,
								_ => KeyType::Public,
							},
							Some(vec![selected_key.get_id()]),
						) {
							Ok(key) => str::from_utf8(&key)
								.map(|v| v.to_string())
								.map_err(AnyhowError::from),
							Err(e) => Err(e),
						}
					}
					CopyType::KeyId => Ok(selected_key.get_id()),
					CopyType::KeyFingerprint => {
						Ok(selected_key.get_fingerprint())
					}
					CopyType::KeyUserId => Ok(selected_key.get_user_id()),
				};
				match content {
					Ok(content) => {
						if let Some(clipboard) = self.clipboard.as_mut() {
							clipboard
								.set_contents(content)
								.expect("failed to set clipboard contents");
							self.prompt.set_output((
								OutputType::Success,
								format!("{} copied to clipboard", copy_type),
							));
						} else {
							self.prompt.set_output((
								OutputType::Failure,
								String::from("clipboard not available"),
							));
						}
					}
					Err(e) => {
						self.prompt.set_output((
							OutputType::Failure,
							format!("copy error: {}", e),
						));
					}
				}
				self.mode = Mode::Normal;
			}
			Command::Paste => {
				if let Some(clipboard) = self.clipboard.as_mut() {
					self.prompt.clear();
					self.prompt.text = format!(
						":{}",
						clipboard
							.get_contents()
							.expect("failed to get clipboard contents")
					);
				} else {
					self.prompt.set_output((
						OutputType::Failure,
						String::from("clipboard not available"),
					));
				}
			}
			Command::EnableInput => self.prompt.enable_command_input(),
			Command::Search(query) => {
				self.prompt.text = format!("/{}", query.unwrap_or_default());
				self.prompt.enable_search();
				self.keys_table.items = self.keys_table.default_items.clone();
			}
			Command::NextTab => {
				self.run_command(self.tab.next().get_command())?
			}
			Command::PreviousTab => {
				self.run_command(self.tab.previous().get_command())?
			}
			Command::Refresh => self.refresh()?,
			Command::Quit => self.state.running = false,
			Command::Confirm(_) | Command::None => {}
		}
		self.state.show_options = show_options;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::gpg::config::GpgConfig;
	#[test]
	fn test_app_launcher() -> Result<()> {
		let args = Args::default();
		let config = GpgConfig::new(&args)?;
		let mut context = GpgContext::new(config)?;
		let mut app = App::new(&mut context, &args)?;
		app.refresh()?;
		Ok(())
	}
}
