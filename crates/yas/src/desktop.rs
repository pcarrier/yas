//! YAS Desktop family version 1 payload codecs.

use crate::prelude::*;

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, limit_u32, put_len_u32,
    put_string_u16, put_string_u32, put_u16, put_u32, put_u64, read_limit_u32,
    reject_unknown_required_extensions,
};
use crate::state::{Record, RecordKind, Watch as StateWatch};
use crate::transfer::{Delivery, Descriptor, Direction, InlineOrTransfer, Mode};

pub const VERSION: u16 = crate::schema::desktop::VERSION;

pub mod request_kind {
    pub use crate::schema::desktop::request::*;
}

pub mod event_kind {
    pub use crate::schema::desktop::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_tray_items: u32,
    pub max_notifications: u32,
    pub max_menu_nodes: u32,
    pub max_notification_actions: u32,
    pub max_inline_menu_bytes: u32,
    pub max_inline_asset_bytes: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_tray_items: crate::schema::desktop::MAX_TRAY_ITEMS as u32,
        max_notifications: crate::schema::desktop::MAX_NOTIFICATIONS as u32,
        max_menu_nodes: crate::schema::desktop::MAX_MENU_NODES as u32,
        max_notification_actions: crate::schema::desktop::MAX_NOTIFICATION_ACTIONS as u32,
        max_inline_menu_bytes: crate::schema::desktop::MAX_INLINE_MENU_BYTES as u32,
        max_inline_asset_bytes: crate::schema::desktop::MAX_INLINE_ASSET_BYTES as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let valid = |value: u32, maximum: u32| value != 0 && value <= maximum;
        if !valid(self.max_tray_items, hard.max_tray_items)
            || !valid(self.max_notifications, hard.max_notifications)
            || !valid(self.max_menu_nodes, hard.max_menu_nodes)
            || !valid(self.max_notification_actions, hard.max_notification_actions)
            || !valid(self.max_inline_menu_bytes, hard.max_inline_menu_bytes)
            || !valid(self.max_inline_asset_bytes, hard.max_inline_asset_bytes)
        {
            return Err(Error::Invalid("Desktop family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::desktop::LIMIT_MAX_TRAY_ITEMS,
                self.max_tray_items,
            ),
            limit_u32(
                crate::schema::desktop::LIMIT_MAX_NOTIFICATIONS,
                self.max_notifications,
            ),
            limit_u32(
                crate::schema::desktop::LIMIT_MAX_MENU_NODES,
                self.max_menu_nodes,
            ),
            limit_u32(
                crate::schema::desktop::LIMIT_MAX_NOTIFICATION_ACTIONS,
                self.max_notification_actions,
            ),
            limit_u32(
                crate::schema::desktop::LIMIT_MAX_INLINE_MENU_BYTES,
                self.max_inline_menu_bytes,
            ),
            limit_u32(
                crate::schema::desktop::LIMIT_MAX_INLINE_ASSET_BYTES,
                self.max_inline_asset_bytes,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required_extensions(
            extensions,
            &[
                crate::schema::desktop::LIMIT_MAX_TRAY_ITEMS as u16,
                crate::schema::desktop::LIMIT_MAX_NOTIFICATIONS as u16,
                crate::schema::desktop::LIMIT_MAX_MENU_NODES as u16,
                crate::schema::desktop::LIMIT_MAX_NOTIFICATION_ACTIONS as u16,
                crate::schema::desktop::LIMIT_MAX_INLINE_MENU_BYTES as u16,
                crate::schema::desktop::LIMIT_MAX_INLINE_ASSET_BYTES as u16,
            ],
            "unknown required Desktop family limit",
        )?;
        let value = Self {
            max_tray_items: read_limit_u32(
                extensions,
                crate::schema::desktop::LIMIT_MAX_TRAY_ITEMS,
            )?,
            max_notifications: read_limit_u32(
                extensions,
                crate::schema::desktop::LIMIT_MAX_NOTIFICATIONS,
            )?,
            max_menu_nodes: read_limit_u32(
                extensions,
                crate::schema::desktop::LIMIT_MAX_MENU_NODES,
            )?,
            max_notification_actions: read_limit_u32(
                extensions,
                crate::schema::desktop::LIMIT_MAX_NOTIFICATION_ACTIONS,
            )?,
            max_inline_menu_bytes: read_limit_u32(
                extensions,
                crate::schema::desktop::LIMIT_MAX_INLINE_MENU_BYTES,
            )?,
            max_inline_asset_bytes: read_limit_u32(
                extensions,
                crate::schema::desktop::LIMIT_MAX_INLINE_ASSET_BYTES,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn handle(value: u64, name: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(name))
    } else {
        Ok(())
    }
}

fn revision(value: u64) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid("zero Desktop revision"))
    } else {
        Ok(())
    }
}

fn operation_id(value: &[u8; 16]) -> Result<()> {
    if *value == [0; 16] {
        Err(Error::Invalid("zero Desktop operation ID"))
    } else {
        Ok(())
    }
}

fn extension(extensions: &Extensions, tag: u64) -> Option<&Extension> {
    extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
}

fn validate_no_required_extensions(extensions: &Extensions, context: &'static str) -> Result<()> {
    extensions.validate()?;
    if extensions.0.iter().any(|extension| extension.required) {
        return Err(Error::Invalid(context));
    }
    Ok(())
}

fn validate_transfer(descriptor: &Descriptor, content_kind: u16) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Byte
        || descriptor.direction != Direction::SENDER_TO_RECEIVER
        || descriptor.content_family != crate::family::DESKTOP
        || descriptor.content_kind != content_kind
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("Desktop Transfer descriptor"));
    }
    Ok(())
}

pub fn watch_datasets(watch: &StateWatch) -> Result<u8> {
    watch.extensions.validate()?;
    let Some(extension) =
        watch.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::desktop::WATCH_DATASET_EXTENSION as u16
        })
    else {
        return Ok(
            (crate::schema::desktop::WATCH_TRAY | crate::schema::desktop::WATCH_NOTIFICATIONS)
                as u8,
        );
    };
    let [datasets] = extension.value.as_slice() else {
        return Err(Error::Invalid("Desktop WATCH dataset extension"));
    };
    let known =
        (crate::schema::desktop::WATCH_TRAY | crate::schema::desktop::WATCH_NOTIFICATIONS) as u8;
    if *datasets == 0 || *datasets & !known != 0 {
        return Err(Error::Invalid("Desktop WATCH datasets"));
    }
    Ok(*datasets)
}

pub fn watch_datasets_extension(datasets: u8) -> Result<Extension> {
    let known =
        (crate::schema::desktop::WATCH_TRAY | crate::schema::desktop::WATCH_NOTIFICATIONS) as u8;
    if datasets == 0 || datasets & !known != 0 {
        return Err(Error::Invalid("Desktop WATCH datasets"));
    }
    Ok(Extension {
        tag: crate::schema::desktop::WATCH_DATASET_EXTENSION as u16,
        required: false,
        value: vec![datasets],
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GetMenu {
    pub tray_handle: u64,
    pub tray_revision: u64,
    pub menu_revision: u64,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for GetMenu {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.tray_handle, "zero Desktop tray handle")?;
        revision(self.tray_revision)?;
        revision(self.menu_revision)?;
        put_u64(out, self.tray_handle);
        put_u64(out, self.tray_revision);
        put_u64(out, self.menu_revision);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for GetMenu {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            tray_handle: decoder.u64()?,
            tray_revision: decoder.u64()?,
            menu_revision: decoder.u64()?,
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuResult(pub InlineOrTransfer);

impl MenuResult {
    fn validate(&self) -> Result<()> {
        match &self.0.delivery {
            Delivery::Inline(bytes) => {
                if bytes.len() > crate::schema::desktop::MAX_INLINE_MENU_BYTES as usize {
                    return Err(Error::LimitExceeded {
                        limit: "Desktop inline menu bytes",
                        actual: bytes.len() as u64,
                        maximum: crate::schema::desktop::MAX_INLINE_MENU_BYTES,
                    });
                }
                MenuTree::decode(bytes)?;
            }
            Delivery::Transfer(descriptor) => {
                validate_transfer(descriptor, crate::schema::desktop::MENU_CONTENT_KIND as u16)?
            }
        }
        Ok(())
    }
}

impl Encode for MenuResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        self.0.encode_to(out)
    }
}

impl Decode for MenuResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let value = Self(InlineOrTransfer::decode(input)?);
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayAction {
    pub tray_handle: u64,
    pub tray_revision: u64,
    pub menu_revision: u64,
    pub operation_id: [u8; 16],
    pub action_kind: u8,
    pub flags: u8,
    pub value: i32,
    pub item_handle: u64,
    pub extensions: Extensions,
}

impl Encode for TrayAction {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.tray_handle, "zero Desktop tray handle")?;
        revision(self.tray_revision)?;
        operation_id(&self.operation_id)?;
        let menu_item = self.action_kind == crate::schema::desktop::TRAY_ACTION_MENU_ITEM as u8;
        let scroll = self.action_kind == crate::schema::desktop::TRAY_ACTION_SCROLL as u8;
        if self.action_kind > crate::schema::desktop::TRAY_ACTION_MENU_ITEM as u8
            || self.flags & !(crate::schema::desktop::TRAY_ACTION_FLAGS_MASK as u8) != 0
            || (!scroll && (self.flags != 0 || self.value != 0))
            || (scroll && self.value == 0)
            || (menu_item != (self.item_handle != 0))
            || (menu_item != (self.menu_revision != 0))
        {
            return Err(Error::Invalid("Desktop tray action shape"));
        }
        put_u64(out, self.tray_handle);
        put_u64(out, self.tray_revision);
        put_u64(out, self.menu_revision);
        out.extend_from_slice(&self.operation_id);
        out.push(self.action_kind);
        out.push(self.flags);
        put_u16(out, 0);
        out.extend_from_slice(&self.value.to_le_bytes());
        put_u64(out, self.item_handle);
        self.extensions.encode_tail(out)
    }
}

impl Decode for TrayAction {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            tray_handle: decoder.u64()?,
            tray_revision: decoder.u64()?,
            menu_revision: decoder.u64()?,
            operation_id: decoder.array_16()?,
            action_kind: decoder.u8()?,
            flags: decoder.u8()?,
            value: {
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Desktop tray action reserved field"));
                }
                i32::from_le_bytes(
                    decoder
                        .take(4)?
                        .try_into()
                        .map_err(|_| Error::Invalid("Desktop tray action value"))?,
                )
            },
            item_handle: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationAction {
    pub notification_handle: u64,
    pub revision: u64,
    pub action_kind: u8,
    pub action_handle: u64,
    pub operation_id: [u8; 16],
    pub reply: String,
    pub extensions: Extensions,
}

impl Encode for NotificationAction {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.notification_handle, "zero Desktop notification handle")?;
        revision(self.revision)?;
        operation_id(&self.operation_id)?;
        let action = self.action_kind == crate::schema::desktop::NOTIFICATION_ACTION_ACTION as u8;
        if self.action_kind > crate::schema::desktop::NOTIFICATION_ACTION_DISMISS as u8
            || action != (self.action_handle != 0)
            || (!action && !self.reply.is_empty())
        {
            return Err(Error::Invalid("Desktop notification action shape"));
        }
        put_u64(out, self.notification_handle);
        put_u64(out, self.revision);
        out.push(self.action_kind);
        out.extend_from_slice(&[0; 3]);
        put_u64(out, self.action_handle);
        out.extend_from_slice(&self.operation_id);
        put_string_u32(out, &self.reply)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for NotificationAction {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let notification_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let action_kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Desktop notification action reserved bytes"));
        }
        let value = Self {
            notification_handle,
            revision,
            action_kind,
            action_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            reply: decoder.string_u32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

pub type InvokeNotificationAction = NotificationAction;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchAsset {
    pub content_hash: [u8; 32],
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for FetchAsset {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for FetchAsset {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            content_hash: decoder.array_32()?,
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetResult(pub InlineOrTransfer);

impl AssetResult {
    fn validate(&self) -> Result<()> {
        match &self.0.delivery {
            Delivery::Inline(bytes)
                if bytes.len() <= crate::schema::desktop::MAX_INLINE_ASSET_BYTES as usize =>
            {
                Ok(())
            }
            Delivery::Inline(bytes) => Err(Error::LimitExceeded {
                limit: "Desktop inline asset bytes",
                actual: bytes.len() as u64,
                maximum: crate::schema::desktop::MAX_INLINE_ASSET_BYTES,
            }),
            Delivery::Transfer(descriptor) => validate_transfer(
                descriptor,
                crate::schema::desktop::ASSET_CONTENT_KIND as u16,
            ),
        }
    }
}

impl Encode for AssetResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        self.0.encode_to(out)
    }
}

impl Decode for AssetResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let value = Self(InlineOrTransfer::decode(input)?);
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuNode {
    pub node_handle: u64,
    pub parent_handle: u64,
    pub kind: u8,
    pub flags: u8,
    pub position: u32,
    pub action_handle: u64,
    pub label: String,
    pub shortcut: String,
    pub icon_hash: [u8; 32],
    pub extensions: Extensions,
}

impl MenuNode {
    fn validate(&self) -> Result<()> {
        handle(self.node_handle, "zero Desktop menu node handle")?;
        if self.kind > crate::schema::desktop::MENU_NODE_SUBMENU as u8
            || self.flags & !(crate::schema::desktop::MENU_FLAGS_MASK as u8) != 0
        {
            return Err(Error::Invalid("Desktop menu node kind or flags"));
        }
        let root = self.kind == crate::schema::desktop::MENU_NODE_ROOT as u8;
        let separator = self.kind == crate::schema::desktop::MENU_NODE_SEPARATOR as u8;
        if root != (self.parent_handle == 0)
            || (root && self.action_handle != 0)
            || (separator && (!self.label.is_empty() || self.action_handle != 0))
        {
            return Err(Error::Invalid("Desktop menu node shape"));
        }
        self.extensions.validate()
    }
}

impl Encode for MenuNode {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.node_handle);
        put_u64(out, self.parent_handle);
        out.push(self.kind);
        out.push(self.flags);
        put_u16(out, 0);
        put_u32(out, self.position);
        put_u64(out, self.action_handle);
        put_string_u16(out, &self.label)?;
        put_string_u16(out, &self.shortcut)?;
        out.extend_from_slice(&self.icon_hash);
        self.extensions.encode_tail(out)
    }
}

impl Decode for MenuNode {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let node_handle = decoder.u64()?;
        let parent_handle = decoder.u64()?;
        let kind = decoder.u8()?;
        let flags = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Desktop menu node reserved field"));
        }
        let value = Self {
            node_handle,
            parent_handle,
            kind,
            flags,
            position: decoder.u32()?,
            action_handle: decoder.u64()?,
            label: decoder.string_u16()?,
            shortcut: decoder.string_u16()?,
            icon_hash: decoder.array_32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuTree {
    pub tray_handle: u64,
    pub tray_revision: u64,
    pub menu_revision: u64,
    pub nodes: Vec<MenuNode>,
    pub extensions: Extensions,
}

impl MenuTree {
    fn validate(&self) -> Result<()> {
        handle(self.tray_handle, "zero Desktop tray handle")?;
        revision(self.tray_revision)?;
        revision(self.menu_revision)?;
        if self.nodes.is_empty()
            || self.nodes.len() > crate::schema::desktop::MAX_MENU_NODES as usize
        {
            return Err(Error::Invalid("Desktop menu node count"));
        }
        let mut seen = BTreeSet::new();
        for (index, node) in self.nodes.iter().enumerate() {
            node.validate()?;
            if !seen.insert(node.node_handle)
                || (index == 0 && node.kind != crate::schema::desktop::MENU_NODE_ROOT as u8)
                || (index != 0 && !seen.contains(&node.parent_handle))
            {
                return Err(Error::Invalid("Desktop menu tree order"));
            }
        }
        self.extensions.validate()
    }
}

impl Encode for MenuTree {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.tray_handle);
        put_u64(out, self.tray_revision);
        put_u64(out, self.menu_revision);
        put_u32(out, self.nodes.len() as u32);
        for node in &self.nodes {
            let bytes = node.encode()?;
            put_len_u32(out, bytes.len())?;
            out.extend_from_slice(&bytes);
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for MenuTree {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let tray_handle = decoder.u64()?;
        let tray_revision = decoder.u64()?;
        let menu_revision = decoder.u64()?;
        let count = decoder.u32()? as usize;
        if count == 0
            || count > crate::schema::desktop::MAX_MENU_NODES as usize
            || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("Desktop menu node count"));
        }
        let mut nodes = Vec::with_capacity(count);
        for _ in 0..count {
            nodes.push(MenuNode::decode(decoder.len_bytes_u32()?)?);
        }
        let value = Self {
            tray_handle,
            tray_revision,
            menu_revision,
            nodes,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayRecord {
    pub tray_handle: u64,
    pub revision: u64,
    pub menu_revision: u64,
    pub status: u8,
    pub title: String,
    pub icon_hash: [u8; 32],
    pub extensions: Extensions,
}

impl TrayRecord {
    fn validate(&self) -> Result<()> {
        handle(self.tray_handle, "zero Desktop tray handle")?;
        revision(self.revision)?;
        revision(self.menu_revision)?;
        if self.status > crate::schema::desktop::TRAY_STATUS_NEEDS_ATTENTION as u8 {
            return Err(Error::Invalid("Desktop tray status"));
        }
        validate_no_required_extensions(&self.extensions, "unknown required Desktop tray extension")
    }
}

impl Encode for TrayRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.tray_handle);
        put_u64(out, self.revision);
        put_u64(out, self.menu_revision);
        out.push(self.status);
        out.extend_from_slice(&[0; 3]);
        put_string_u16(out, &self.title)?;
        out.extend_from_slice(&self.icon_hash);
        self.extensions.encode_tail(out)
    }
}

impl Decode for TrayRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let tray_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let menu_revision = decoder.u64()?;
        let status = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Desktop tray reserved bytes"));
        }
        let value = Self {
            tray_handle,
            revision,
            menu_revision,
            status,
            title: decoder.string_u16()?,
            icon_hash: decoder.array_32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationButton {
    pub action_handle: u64,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationProgress {
    pub value: u32,
    pub maximum: u32,
}

impl NotificationProgress {
    fn validate(self) -> Result<()> {
        if self.maximum == 0 || self.value > self.maximum {
            return Err(Error::Invalid("Desktop notification progress"));
        }
        Ok(())
    }

    pub fn extension(self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::desktop::NOTIFICATION_PROGRESS_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for NotificationProgress {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.value);
        put_u32(out, self.maximum);
        Ok(())
    }
}

impl Decode for NotificationProgress {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            value: decoder.u32()?,
            maximum: decoder.u32()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationReply {
    pub placeholder: String,
}

impl NotificationReply {
    pub fn extension(&self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::desktop::NOTIFICATION_REPLY_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for NotificationReply {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        put_string_u16(out, &self.placeholder)
    }
}

impl Decode for NotificationReply {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            placeholder: decoder.string_u16()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

pub fn notification_content_image_hash_extension(content_hash: [u8; 32]) -> Extension {
    Extension {
        tag: crate::schema::desktop::NOTIFICATION_IMAGE_HASH_EXTENSION as u16,
        required: false,
        value: content_hash.to_vec(),
    }
}

pub fn notification_application_icon_hash_extension(content_hash: [u8; 32]) -> Extension {
    Extension {
        tag: crate::schema::desktop::NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION as u16,
        required: false,
        value: content_hash.to_vec(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationFieldPatch<T> {
    Clear,
    Set(T),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotificationPatchMetadata {
    pub content_image_hash: Option<NotificationFieldPatch<[u8; 32]>>,
    pub application_icon_hash: Option<NotificationFieldPatch<[u8; 32]>>,
    pub progress: Option<NotificationFieldPatch<NotificationProgress>>,
    pub reply: Option<NotificationFieldPatch<NotificationReply>>,
}

impl NotificationPatchMetadata {
    pub fn to_extensions(&self) -> Result<Extensions> {
        let mut extensions = Vec::new();
        if let Some(value) = &self.content_image_hash {
            extensions.push(hash_patch_extension(
                crate::schema::desktop::NOTIFICATION_IMAGE_HASH_EXTENSION,
                value,
            ));
        }
        if let Some(value) = &self.application_icon_hash {
            extensions.push(hash_patch_extension(
                crate::schema::desktop::NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION,
                value,
            ));
        }
        if let Some(value) = &self.progress {
            extensions.push(Extension {
                tag: crate::schema::desktop::NOTIFICATION_PROGRESS_EXTENSION as u16,
                required: false,
                value: match value {
                    NotificationFieldPatch::Clear => Vec::new(),
                    NotificationFieldPatch::Set(value) => value.encode()?,
                },
            });
        }
        if let Some(value) = &self.reply {
            extensions.push(Extension {
                tag: crate::schema::desktop::NOTIFICATION_REPLY_EXTENSION as u16,
                required: false,
                value: match value {
                    NotificationFieldPatch::Clear => Vec::new(),
                    NotificationFieldPatch::Set(value) => value.encode()?,
                },
            });
        }
        let extensions = Extensions(extensions);
        validate_notification_extensions(&extensions, true)?;
        Ok(extensions)
    }
}

fn hash_patch_extension(tag: u64, value: &NotificationFieldPatch<[u8; 32]>) -> Extension {
    Extension {
        tag: tag as u16,
        required: false,
        value: match value {
            NotificationFieldPatch::Clear => Vec::new(),
            NotificationFieldPatch::Set(value) => value.to_vec(),
        },
    }
}

fn decode_hash(value: &[u8], context: &'static str) -> Result<[u8; 32]> {
    value.try_into().map_err(|_| Error::Invalid(context))
}

fn decode_hash_patch(
    value: &[u8],
    context: &'static str,
) -> Result<NotificationFieldPatch<[u8; 32]>> {
    if value.is_empty() {
        Ok(NotificationFieldPatch::Clear)
    } else {
        Ok(NotificationFieldPatch::Set(decode_hash(value, context)?))
    }
}

fn decode_typed_patch<T: Decode>(value: &[u8]) -> Result<NotificationFieldPatch<T>> {
    if value.is_empty() {
        Ok(NotificationFieldPatch::Clear)
    } else {
        Ok(NotificationFieldPatch::Set(T::decode(value)?))
    }
}

fn validate_notification_extensions(extensions: &Extensions, patch: bool) -> Result<()> {
    extensions.validate()?;
    for extension in &extensions.0 {
        if patch && extension.value.is_empty() {
            match extension.tag {
                tag if tag == crate::schema::desktop::NOTIFICATION_IMAGE_HASH_EXTENSION as u16
                    || tag
                        == crate::schema::desktop::NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION
                            as u16
                    || tag == crate::schema::desktop::NOTIFICATION_PROGRESS_EXTENSION as u16
                    || tag == crate::schema::desktop::NOTIFICATION_REPLY_EXTENSION as u16 =>
                {
                    continue;
                }
                _ => {}
            }
        }
        match extension.tag {
            tag if tag == crate::schema::desktop::NOTIFICATION_IMAGE_HASH_EXTENSION as u16 => {
                decode_hash(&extension.value, "Desktop notification content image hash")?;
            }
            tag if tag
                == crate::schema::desktop::NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION as u16 =>
            {
                decode_hash(
                    &extension.value,
                    "Desktop notification application icon hash",
                )?;
            }
            tag if tag == crate::schema::desktop::NOTIFICATION_PROGRESS_EXTENSION as u16 => {
                NotificationProgress::decode(&extension.value)?;
            }
            tag if tag == crate::schema::desktop::NOTIFICATION_REPLY_EXTENSION as u16 => {
                NotificationReply::decode(&extension.value)?;
            }
            _ if extension.required => {
                return Err(Error::Invalid(
                    "unknown required Desktop notification extension",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationRecord {
    pub notification_handle: u64,
    pub revision: u64,
    pub flags: u16,
    pub urgency: u8,
    pub expires_server_ns: u64,
    pub application: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<NotificationButton>,
    pub extensions: Extensions,
}

impl NotificationRecord {
    fn validate(&self) -> Result<()> {
        handle(self.notification_handle, "zero Desktop notification handle")?;
        revision(self.revision)?;
        if self.flags & !(crate::schema::desktop::NOTIFICATION_FLAGS_MASK as u16) != 0
            || self.urgency > crate::schema::desktop::NOTIFICATION_URGENCY_CRITICAL as u8
            || self.actions.len() > crate::schema::desktop::MAX_NOTIFICATION_ACTIONS as usize
        {
            return Err(Error::Invalid("Desktop notification flags or count"));
        }
        let mut handles = BTreeSet::new();
        for action in &self.actions {
            if action.action_handle == 0 || !handles.insert(action.action_handle) {
                return Err(Error::Invalid("Desktop notification action"));
            }
        }
        validate_notification_extensions(&self.extensions, false)?;
        let has_progress = extension(
            &self.extensions,
            crate::schema::desktop::NOTIFICATION_PROGRESS_EXTENSION,
        )
        .is_some();
        let has_reply = extension(
            &self.extensions,
            crate::schema::desktop::NOTIFICATION_REPLY_EXTENSION,
        )
        .is_some();
        if has_progress
            != (self.flags & crate::schema::desktop::NOTIFICATION_HAS_PROGRESS as u16 != 0)
            || has_reply
                != (self.flags & crate::schema::desktop::NOTIFICATION_HAS_REPLY as u16 != 0)
        {
            return Err(Error::Invalid("Desktop notification metadata flags"));
        }
        Ok(())
    }

    pub fn content_image_hash(&self) -> Result<Option<[u8; 32]>> {
        extension(
            &self.extensions,
            crate::schema::desktop::NOTIFICATION_IMAGE_HASH_EXTENSION,
        )
        .map(|extension| decode_hash(&extension.value, "Desktop notification content image hash"))
        .transpose()
    }

    pub fn application_icon_hash(&self) -> Result<Option<[u8; 32]>> {
        extension(
            &self.extensions,
            crate::schema::desktop::NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION,
        )
        .map(|extension| {
            decode_hash(
                &extension.value,
                "Desktop notification application icon hash",
            )
        })
        .transpose()
    }

    pub fn progress(&self) -> Result<Option<NotificationProgress>> {
        extension(
            &self.extensions,
            crate::schema::desktop::NOTIFICATION_PROGRESS_EXTENSION,
        )
        .map(|extension| NotificationProgress::decode(&extension.value))
        .transpose()
    }

    pub fn reply(&self) -> Result<Option<NotificationReply>> {
        extension(
            &self.extensions,
            crate::schema::desktop::NOTIFICATION_REPLY_EXTENSION,
        )
        .map(|extension| NotificationReply::decode(&extension.value))
        .transpose()
    }
}

impl Encode for NotificationRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.notification_handle);
        put_u64(out, self.revision);
        put_u16(out, self.flags);
        out.push(self.urgency);
        out.push(0);
        put_u64(out, self.expires_server_ns);
        put_string_u16(out, &self.application)?;
        put_string_u16(out, &self.summary)?;
        put_string_u32(out, &self.body)?;
        put_u16(out, self.actions.len() as u16);
        for action in &self.actions {
            put_u64(out, action.action_handle);
            put_string_u16(out, &action.label)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for NotificationRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let notification_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let flags = decoder.u16()?;
        let urgency = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Desktop notification reserved byte"));
        }
        let expires_server_ns = decoder.u64()?;
        let application = decoder.string_u16()?;
        let summary = decoder.string_u16()?;
        let body = decoder.string_u32()?;
        let count = usize::from(decoder.u16()?);
        if count > crate::schema::desktop::MAX_NOTIFICATION_ACTIONS as usize
            || count > decoder.remaining() / 10
        {
            return Err(Error::Invalid("Desktop notification action count"));
        }
        let mut actions = Vec::with_capacity(count);
        for _ in 0..count {
            actions.push(NotificationButton {
                action_handle: decoder.u64()?,
                label: decoder.string_u16()?,
            });
        }
        let value = Self {
            notification_handle,
            revision,
            flags,
            urgency,
            expires_server_ns,
            application,
            summary,
            body,
            actions,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompleteEntity {
    Tray(TrayRecord),
    Notification(NotificationRecord),
}

impl CompleteEntity {
    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Desktop complete state record kind"));
        }
        let mut body = Vec::new();
        match self {
            Self::Tray(record) => {
                put_u16(&mut body, crate::schema::desktop::RECORD_TRAY as u16);
                put_u16(&mut body, 0);
                record.encode_to(&mut body)?;
            }
            Self::Notification(record) => {
                put_u16(
                    &mut body,
                    crate::schema::desktop::RECORD_NOTIFICATION as u16,
                );
                put_u16(&mut body, 0);
                record.encode_to(&mut body)?;
            }
        }
        Ok(Record {
            kind,
            required: false,
            body,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityPatch {
    pub entity_kind: u16,
    pub handle: u64,
    pub revision: u64,
    pub extensions: Extensions,
}

impl EntityPatch {
    pub fn state_record(&self) -> Result<Record> {
        validate_entity(self.entity_kind, self.handle, self.revision)?;
        if self.entity_kind == crate::schema::desktop::RECORD_NOTIFICATION as u16 {
            validate_notification_extensions(&self.extensions, true)?;
        } else {
            validate_no_required_extensions(
                &self.extensions,
                "unknown required Desktop tray patch extension",
            )?;
        }
        let mut body = Vec::new();
        put_u16(&mut body, self.entity_kind);
        put_u16(&mut body, 0);
        put_u64(&mut body, self.handle);
        put_u64(&mut body, self.revision);
        self.extensions.encode_tail(&mut body)?;
        Ok(Record {
            kind: RecordKind::Patch,
            required: false,
            body,
        })
    }

    pub fn notification_metadata(&self) -> Result<NotificationPatchMetadata> {
        if self.entity_kind != crate::schema::desktop::RECORD_NOTIFICATION as u16 {
            return Err(Error::Invalid("Desktop notification patch entity"));
        }
        validate_notification_extensions(&self.extensions, true)?;
        let mut value = NotificationPatchMetadata::default();
        for extension in &self.extensions.0 {
            match extension.tag {
                tag if tag == crate::schema::desktop::NOTIFICATION_IMAGE_HASH_EXTENSION as u16 => {
                    value.content_image_hash = Some(decode_hash_patch(
                        &extension.value,
                        "Desktop notification content image hash",
                    )?);
                }
                tag if tag
                    == crate::schema::desktop::NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION
                        as u16 =>
                {
                    value.application_icon_hash = Some(decode_hash_patch(
                        &extension.value,
                        "Desktop notification application icon hash",
                    )?);
                }
                tag if tag == crate::schema::desktop::NOTIFICATION_PROGRESS_EXTENSION as u16 => {
                    value.progress = Some(decode_typed_patch(&extension.value)?);
                }
                tag if tag == crate::schema::desktop::NOTIFICATION_REPLY_EXTENSION as u16 => {
                    value.reply = Some(decode_typed_patch(&extension.value)?);
                }
                _ => {}
            }
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovedEntity {
    Tray {
        handle: u64,
        revision: u64,
    },
    Notification {
        handle: u64,
        revision: u64,
        close_reason: u8,
    },
}

impl RemovedEntity {
    pub fn state_record(self) -> Result<Record> {
        let mut body = Vec::new();
        match self {
            Self::Tray { handle, revision } => {
                validate_entity(crate::schema::desktop::RECORD_TRAY as u16, handle, revision)?;
                put_u16(&mut body, crate::schema::desktop::RECORD_TRAY as u16);
                put_u16(&mut body, 0);
                put_u64(&mut body, handle);
                put_u64(&mut body, revision);
            }
            Self::Notification {
                handle,
                revision,
                close_reason,
            } => {
                validate_entity(
                    crate::schema::desktop::RECORD_NOTIFICATION as u16,
                    handle,
                    revision,
                )?;
                validate_notification_close_reason(close_reason)?;
                put_u16(
                    &mut body,
                    crate::schema::desktop::RECORD_NOTIFICATION as u16,
                );
                put_u16(&mut body, 0);
                put_u64(&mut body, handle);
                put_u64(&mut body, revision);
                body.push(close_reason);
                body.extend_from_slice(&[0; 3]);
            }
        }
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body,
        })
    }
}

fn validate_notification_close_reason(value: u8) -> Result<()> {
    if value < crate::schema::desktop::NOTIFICATION_CLOSED_EXPIRED as u8
        || value > crate::schema::desktop::NOTIFICATION_CLOSED_UNDEFINED as u8
    {
        return Err(Error::Invalid("Desktop notification close reason"));
    }
    Ok(())
}

fn validate_entity(entity_kind: u16, value: u64, value_revision: u64) -> Result<()> {
    if entity_kind != crate::schema::desktop::RECORD_TRAY as u16
        && entity_kind != crate::schema::desktop::RECORD_NOTIFICATION as u16
    {
        return Err(Error::Invalid("Desktop state entity"));
    }
    handle(value, "zero Desktop entity handle")?;
    revision(value_revision)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateMutation {
    Complete(CompleteEntity),
    Patch(EntityPatch),
    Remove(RemovedEntity),
}

pub fn decode_state_record(record: &Record) -> Result<StateMutation> {
    let mut decoder = Decoder::new(&record.body);
    let entity_kind = decoder.u16()?;
    if decoder.u16()? != 0 {
        return Err(Error::Invalid("Desktop state entity reserved field"));
    }
    let payload = decoder.rest();
    decoder.finish()?;
    match record.kind {
        RecordKind::Add | RecordKind::Replace => match entity_kind {
            value if value == crate::schema::desktop::RECORD_TRAY as u16 => Ok(
                StateMutation::Complete(CompleteEntity::Tray(TrayRecord::decode(payload)?)),
            ),
            value if value == crate::schema::desktop::RECORD_NOTIFICATION as u16 => {
                Ok(StateMutation::Complete(CompleteEntity::Notification(
                    NotificationRecord::decode(payload)?,
                )))
            }
            _ => Err(Error::Invalid("Desktop state entity")),
        },
        RecordKind::Patch => {
            let mut value = Decoder::new(payload);
            let patch = EntityPatch {
                entity_kind,
                handle: value.u64()?,
                revision: value.u64()?,
                extensions: value.extensions()?,
            };
            value.finish()?;
            patch.state_record()?;
            Ok(StateMutation::Patch(patch))
        }
        RecordKind::Remove => {
            let mut value = Decoder::new(payload);
            let handle = value.u64()?;
            let revision = value.u64()?;
            let removed = if entity_kind == crate::schema::desktop::RECORD_TRAY as u16 {
                RemovedEntity::Tray { handle, revision }
            } else if entity_kind == crate::schema::desktop::RECORD_NOTIFICATION as u16 {
                let close_reason = value.u8()?;
                if value.take(3)? != [0; 3] {
                    return Err(Error::Invalid("Desktop notification REMOVE reserved bytes"));
                }
                RemovedEntity::Notification {
                    handle,
                    revision,
                    close_reason,
                }
            } else {
                return Err(Error::Invalid("Desktop state entity"));
            };
            value.finish()?;
            removed.state_record()?;
            Ok(StateMutation::Remove(removed))
        }
        _ => Err(Error::Invalid("Desktop state record kind")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);

        let mut invalid = Limits::HARD;
        invalid.max_notifications = 0;
        assert!(invalid.to_extensions().is_err());

        let mut unknown = extensions;
        unknown.0.push(Extension {
            tag: 99,
            required: true,
            value: Vec::new(),
        });
        assert!(Limits::from_extensions(&unknown).is_err());
    }

    fn truncations<T: Encode + Decode + PartialEq + std::fmt::Debug>(value: &T) {
        let bytes = value.encode().unwrap();
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
        assert_eq!(&T::decode(&bytes).unwrap(), value);
    }

    #[test]
    fn menu_tree_is_preorder_typed_and_truncation_safe() {
        let value = MenuTree {
            tray_handle: 1,
            tray_revision: 2,
            menu_revision: 3,
            nodes: vec![
                MenuNode {
                    node_handle: 10,
                    parent_handle: 0,
                    kind: crate::schema::desktop::MENU_NODE_ROOT as u8,
                    flags: crate::schema::desktop::MENU_VISIBLE as u8,
                    position: 0,
                    action_handle: 0,
                    label: String::new(),
                    shortcut: String::new(),
                    icon_hash: [0; 32],
                    extensions: Extensions::default(),
                },
                MenuNode {
                    node_handle: 11,
                    parent_handle: 10,
                    kind: crate::schema::desktop::MENU_NODE_ITEM as u8,
                    flags: (crate::schema::desktop::MENU_ENABLED
                        | crate::schema::desktop::MENU_VISIBLE) as u8,
                    position: 0,
                    action_handle: 20,
                    label: "Open".into(),
                    shortcut: "Ctrl+O".into(),
                    icon_hash: [1; 32],
                    extensions: Extensions::default(),
                },
            ],
            extensions: Extensions::default(),
        };
        truncations(&value);
    }

    #[test]
    fn state_entity_discriminator_round_trips() {
        let value = CompleteEntity::Tray(TrayRecord {
            tray_handle: 1,
            revision: 2,
            menu_revision: 3,
            status: crate::schema::desktop::TRAY_STATUS_ACTIVE as u8,
            title: "agent".into(),
            icon_hash: [4; 32],
            extensions: Extensions::default(),
        });
        let record = value.state_record(RecordKind::Add).unwrap();
        assert_eq!(
            decode_state_record(&record).unwrap(),
            StateMutation::Complete(value)
        );
    }

    #[test]
    fn notification_metadata_and_patch_are_typed() {
        let progress = NotificationProgress {
            value: 7,
            maximum: 10,
        };
        let reply = NotificationReply {
            placeholder: "Reply".into(),
        };
        let value = NotificationRecord {
            notification_handle: 5,
            revision: 6,
            flags: (crate::schema::desktop::NOTIFICATION_RESIDENT
                | crate::schema::desktop::NOTIFICATION_HAS_PROGRESS
                | crate::schema::desktop::NOTIFICATION_HAS_REPLY) as u16,
            urgency: crate::schema::desktop::NOTIFICATION_URGENCY_NORMAL as u8,
            expires_server_ns: 99,
            application: "sync".into(),
            summary: "Uploading".into(),
            body: "payload".into(),
            actions: vec![NotificationButton {
                action_handle: 8,
                label: "Cancel".into(),
            }],
            extensions: Extensions(vec![
                notification_content_image_hash_extension([1; 32]),
                notification_application_icon_hash_extension([2; 32]),
                progress.extension().unwrap(),
                reply.extension().unwrap(),
            ]),
        };
        truncations(&value);
        assert_eq!(value.content_image_hash().unwrap(), Some([1; 32]));
        assert_eq!(value.application_icon_hash().unwrap(), Some([2; 32]));
        assert_eq!(value.progress().unwrap(), Some(progress));
        assert_eq!(value.reply().unwrap(), Some(reply));

        let mut mismatched = value.clone();
        mismatched.flags &= !(crate::schema::desktop::NOTIFICATION_HAS_REPLY as u16);
        assert!(mismatched.encode().is_err());

        let metadata = NotificationPatchMetadata {
            content_image_hash: Some(NotificationFieldPatch::Clear),
            application_icon_hash: Some(NotificationFieldPatch::Set([3; 32])),
            progress: Some(NotificationFieldPatch::Set(NotificationProgress {
                value: 9,
                maximum: 10,
            })),
            reply: Some(NotificationFieldPatch::Clear),
        };
        let patch = EntityPatch {
            entity_kind: crate::schema::desktop::RECORD_NOTIFICATION as u16,
            handle: 5,
            revision: 7,
            extensions: metadata.to_extensions().unwrap(),
        };
        assert_eq!(patch.notification_metadata().unwrap(), metadata);
        let record = patch.state_record().unwrap();
        for end in 0..record.body.len() {
            let truncated = Record {
                kind: RecordKind::Patch,
                required: false,
                body: record.body[..end].to_vec(),
            };
            assert!(
                decode_state_record(&truncated).is_err(),
                "accepted prefix {end}"
            );
        }
        assert_eq!(
            decode_state_record(&record).unwrap(),
            StateMutation::Patch(patch)
        );
    }

    #[test]
    fn desktop_remove_is_entity_specific_and_preserves_close_reason() {
        for removed in [
            RemovedEntity::Tray {
                handle: 1,
                revision: 2,
            },
            RemovedEntity::Notification {
                handle: 3,
                revision: 4,
                close_reason: crate::schema::desktop::NOTIFICATION_CLOSED_DISMISSED as u8,
            },
        ] {
            let record = removed.state_record().unwrap();
            for end in 0..record.body.len() {
                let truncated = Record {
                    kind: RecordKind::Remove,
                    required: false,
                    body: record.body[..end].to_vec(),
                };
                assert!(
                    decode_state_record(&truncated).is_err(),
                    "accepted prefix {end}"
                );
            }
            assert_eq!(
                decode_state_record(&record).unwrap(),
                StateMutation::Remove(removed)
            );
        }

        assert!(
            RemovedEntity::Notification {
                handle: 3,
                revision: 4,
                close_reason: 0,
            }
            .state_record()
            .is_err()
        );
    }

    #[test]
    fn watch_dataset_extension_defaults_and_validates() {
        let mut watch = StateWatch {
            initial_credit: 1,
            resume: None,
            extensions: Extensions::default(),
        };
        assert_eq!(watch_datasets(&watch).unwrap(), 3);
        watch.extensions = Extensions(vec![watch_datasets_extension(1).unwrap()]);
        assert_eq!(watch_datasets(&watch).unwrap(), 1);
    }

    #[test]
    fn tray_and_notification_action_shapes_are_exact() {
        truncations(&TrayAction {
            tray_handle: 1,
            tray_revision: 2,
            menu_revision: 3,
            operation_id: [4; 16],
            action_kind: crate::schema::desktop::TRAY_ACTION_MENU_ITEM as u8,
            flags: 0,
            value: 0,
            item_handle: 5,
            extensions: Extensions::default(),
        });
        truncations(&TrayAction {
            tray_handle: 1,
            tray_revision: 2,
            menu_revision: 0,
            operation_id: [4; 16],
            action_kind: crate::schema::desktop::TRAY_ACTION_SCROLL as u8,
            flags: crate::schema::desktop::TRAY_ACTION_SCROLL_HORIZONTAL as u8,
            value: -120,
            item_handle: 0,
            extensions: Extensions::default(),
        });
        truncations(&NotificationAction {
            notification_handle: 6,
            revision: 7,
            action_kind: crate::schema::desktop::NOTIFICATION_ACTION_ACTION as u8,
            action_handle: 8,
            operation_id: [9; 16],
            reply: "approved".into(),
            extensions: Extensions::default(),
        });
        truncations(&NotificationAction {
            notification_handle: 6,
            revision: 7,
            action_kind: crate::schema::desktop::NOTIFICATION_ACTION_DISMISS as u8,
            action_handle: 0,
            operation_id: [9; 16],
            reply: String::new(),
            extensions: Extensions::default(),
        });

        let invalid = NotificationAction {
            notification_handle: 6,
            revision: 7,
            action_kind: crate::schema::desktop::NOTIFICATION_ACTION_DEFAULT as u8,
            action_handle: 8,
            operation_id: [9; 16],
            reply: String::new(),
            extensions: Extensions::default(),
        };
        assert!(invalid.encode().is_err());
    }
}
