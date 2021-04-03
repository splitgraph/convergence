use bytes::{Buf, BufMut, BytesMut};
use std::convert::TryFrom;
use std::fmt::Display;
use std::mem::size_of;
use std::{collections::HashMap, convert::TryInto};
use tokio_util::codec::{Decoder, Encoder};

macro_rules! data_types {
	($($name:ident = $oid:expr, $size: expr)*) => {
		#[derive(Debug, Copy, Clone)]
		pub enum DataTypeOid {
			$(
				$name,
			)*
			Unknown(u32),
		}

		impl DataTypeOid {
			pub fn size_bytes(&self) -> i16 {
				match self {
					$(
						Self::$name => $size,
					)*
					Self::Unknown(_) => unimplemented!(),
				}
			}
		}

		impl From<u32> for DataTypeOid {
			fn from(value: u32) -> Self {
				match value {
					$(
						$oid => Self::$name,
					)*
					other => Self::Unknown(other),
				}
			}
		}

		impl From<DataTypeOid> for u32 {
			fn from(value: DataTypeOid) -> Self {
				match value {
					$(
						DataTypeOid::$name => $oid,
					)*
					DataTypeOid::Unknown(other) => other,
				}
			}
		}
	};
}

data_types! {
	Unspecified = 0, 0

	Int2 = 21, 2
	Int4 = 23, 4
	Int8 = 20, 8

	Float4 = 700, 4
	Float8 = 701, 8

	Text = 25, -1
}

#[derive(Debug, Copy, Clone)]
pub enum FormatCode {
	Text = 0,
	Binary = 1,
}

impl TryFrom<i16> for FormatCode {
	type Error = ProtocolError;

	fn try_from(value: i16) -> Result<Self, Self::Error> {
		match value {
			0 => Ok(FormatCode::Text),
			1 => Ok(FormatCode::Binary),
			other => Err(ProtocolError::InvalidFormatCode(other)),
		}
	}
}

#[derive(Debug)]
pub struct Startup {
	pub requested_protocol_version: (i16, i16),
	pub parameters: HashMap<String, String>,
}

#[derive(Debug)]
pub enum Describe {
	Portal(String),
	PreparedStatement(String),
}

#[derive(Debug)]
pub struct Parse {
	pub prepared_statement_name: String,
	pub query: String,
	pub parameter_types: Vec<DataTypeOid>,
}

#[derive(Debug)]
pub enum BindFormat {
	All(FormatCode),
	PerColumn(Vec<FormatCode>),
}

#[derive(Debug)]
pub struct Bind {
	pub portal: String,
	pub prepared_statement_name: String,
	pub result_format: BindFormat,
}

#[derive(Debug)]
pub struct Execute {
	pub portal: String,
	pub max_rows: Option<i32>,
}

#[derive(Debug)]
pub enum ClientMessage {
	Startup(Startup),
	Parse(Parse),
	Describe(Describe),
	Bind(Bind),
	Sync,
	Execute(Execute),
	Query(String),
	Terminate,
}

pub trait BackendMessage: std::fmt::Debug {
	const TAG: u8;

	fn encode(&self, dst: &mut BytesMut);
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlState(pub &'static str);

impl SqlState {
	pub const SUCCESSFUL_COMPLETION: SqlState = SqlState("00000");
	pub const FEATURE_NOT_SUPPORTED: SqlState = SqlState("0A000");
	pub const INVALID_CURSOR_NAME: SqlState = SqlState("34000");
	pub const CONNECTION_EXCEPTION: SqlState = SqlState("08000");
	pub const INVALID_SQL_STATEMENT_NAME: SqlState = SqlState("26000");
	pub const DATA_EXCEPTION: SqlState = SqlState("22000");
	pub const PROTOCOL_VIOLATION: SqlState = SqlState("08P01");
	pub const SYNTAX_ERROR: SqlState = SqlState("42601");
}

#[derive(Debug, Clone, PartialEq)]
pub struct Severity(pub &'static str);

impl Severity {
	pub const ERROR: Severity = Severity("ERROR");
	pub const FATAL: Severity = Severity("FATAL");
}

#[derive(thiserror::Error, Debug, Clone)]
pub struct ErrorResponse {
	pub sql_state: SqlState,
	pub severity: Severity,
	pub message: String,
}

impl ErrorResponse {
	pub fn new(sql_state: SqlState, severity: Severity, message: impl Into<String>) -> Self {
		ErrorResponse {
			sql_state,
			severity,
			message: message.into(),
		}
	}

	pub fn error(sql_state: SqlState, message: impl Into<String>) -> Self {
		Self::new(sql_state, Severity::ERROR, message)
	}

	pub fn fatal(sql_state: SqlState, message: impl Into<String>) -> Self {
		Self::new(sql_state, Severity::FATAL, message)
	}
}

impl Display for ErrorResponse {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "error")
	}
}

impl BackendMessage for ErrorResponse {
	const TAG: u8 = b'E';

	fn encode(&self, dst: &mut BytesMut) {
		dst.put_u8(b'C');
		dst.put_slice(self.sql_state.0.as_bytes());
		dst.put_u8(0);
		dst.put_u8(b'S');
		dst.put_slice(self.severity.0.as_bytes());
		dst.put_u8(0);
		dst.put_u8(b'M');
		dst.put_slice(self.message.as_bytes());
		dst.put_u8(0);

		dst.put_u8(0); // tag
	}
}

#[derive(Debug)]
pub struct ParameterDescription {}

impl BackendMessage for ParameterDescription {
	const TAG: u8 = b't';

	fn encode(&self, dst: &mut BytesMut) {
		dst.put_i16(0);
	}
}

#[derive(Debug, Clone)]
pub struct FieldDescription {
	pub name: String,
	pub data_type: DataTypeOid,
}

#[derive(Debug, Clone)]
pub struct RowDescription {
	pub fields: Vec<FieldDescription>,
	pub format_code: FormatCode,
}

impl BackendMessage for RowDescription {
	const TAG: u8 = b'T';

	fn encode(&self, dst: &mut BytesMut) {
		dst.put_i16(self.fields.len() as i16);
		for field in &self.fields {
			dst.put_slice(field.name.as_bytes());
			dst.put_u8(0);
			dst.put_i32(0); // table oid
			dst.put_i16(0); // column attr number
			dst.put_u32(field.data_type.into());
			dst.put_i16(field.data_type.size_bytes());
			dst.put_i32(-1); // data type modifier
			dst.put_i16(self.format_code as i16);
		}
	}
}

#[derive(Debug)]
pub struct AuthenticationOk;

impl BackendMessage for AuthenticationOk {
	const TAG: u8 = b'R';

	fn encode(&self, dst: &mut BytesMut) {
		dst.put_i32(0);
	}
}

#[derive(Debug)]
pub struct ReadyForQuery;

impl BackendMessage for ReadyForQuery {
	const TAG: u8 = b'Z';

	fn encode(&self, dst: &mut BytesMut) {
		dst.put_u8(b'I');
	}
}

#[derive(Debug)]
pub struct ParseComplete;

impl BackendMessage for ParseComplete {
	const TAG: u8 = b'1';

	fn encode(&self, _dst: &mut BytesMut) {}
}

#[derive(Debug)]
pub struct BindComplete;

impl BackendMessage for BindComplete {
	const TAG: u8 = b'2';

	fn encode(&self, _dst: &mut BytesMut) {}
}

#[derive(Debug)]
pub struct CommandComplete {
	pub command_tag: String,
}

impl BackendMessage for CommandComplete {
	const TAG: u8 = b'C';

	fn encode(&self, dst: &mut BytesMut) {
		dst.put_slice(self.command_tag.as_bytes());
		dst.put_u8(0);
	}
}

#[derive(Debug)]
pub struct ParameterStatus {
	name: String,
	value: String,
}

impl BackendMessage for ParameterStatus {
	const TAG: u8 = b'S';

	fn encode(&self, dst: &mut BytesMut) {
		dst.put_slice(self.name.as_bytes());
		dst.put_u8(0);
		dst.put_slice(self.value.as_bytes());
		dst.put_u8(0);
	}
}

impl ParameterStatus {
	pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
		Self {
			name: name.into(),
			value: value.into(),
		}
	}
}

#[derive(Default, Debug)]
pub struct ConnectionCodec {
	// most state tracking is handled at a higher level
	// however, the actual wire format uses a different header for startup vs normal messages
	// so we need to be able to differentiate inside the decoder
	startup_received: bool,
}

impl ConnectionCodec {
	pub fn new() -> Self {
		Self {
			startup_received: false,
		}
	}
}

#[derive(thiserror::Error, Debug)]
pub enum ProtocolError {
	#[error("io error: {0}")]
	Io(#[from] std::io::Error),
	#[error("utf8 error: {0}")]
	Utf8(#[from] std::string::FromUtf8Error),
	#[error("parsing error")]
	ParserError,
	#[error("invalid message type: {0}")]
	InvalidMessageType(u8),
	#[error("invalid format code: {0}")]
	InvalidFormatCode(i16),
}

// length prefix, two version components
const STARTUP_HEADER_SIZE: usize = size_of::<i32>() + (size_of::<i16>() * 2);
// message tag, length prefix
const MESSAGE_HEADER_SIZE: usize = size_of::<u8>() + size_of::<i32>();

impl Decoder for ConnectionCodec {
	type Item = ClientMessage;
	type Error = ProtocolError;

	fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
		if !self.startup_received {
			if src.len() < STARTUP_HEADER_SIZE {
				return Ok(None);
			}

			let mut header_buf = src.clone();
			let message_len = header_buf.get_i32() as usize;
			let protocol_version_major = header_buf.get_i16();
			let protocol_version_minor = header_buf.get_i16();

			if src.len() < message_len {
				src.reserve(message_len - src.len());
				return Ok(None);
			}

			src.advance(STARTUP_HEADER_SIZE);

			let mut parameters = HashMap::new();

			let mut param_str_start_pos = 0;
			let mut current_key = None;
			for (i, &blah) in src.iter().enumerate() {
				if blah == 0 {
					let string_value = String::from_utf8(src[param_str_start_pos..i].to_owned())?;
					param_str_start_pos = i + 1;

					current_key = match current_key {
						Some(key) => {
							parameters.insert(key, string_value);
							None
						}
						None => Some(string_value),
					}
				}
			}

			src.advance(message_len - STARTUP_HEADER_SIZE);

			self.startup_received = true;
			return Ok(Some(ClientMessage::Startup(Startup {
				requested_protocol_version: (protocol_version_major, protocol_version_minor),
				parameters,
			})));
		}

		if src.len() < MESSAGE_HEADER_SIZE {
			src.reserve(MESSAGE_HEADER_SIZE);
			return Ok(None);
		}

		let mut header_buf = src.clone();
		let message_tag = header_buf.get_u8();
		let message_len = header_buf.get_i32() as usize;

		if src.len() < message_len {
			src.reserve(message_len - src.len());
			return Ok(None);
		}

		src.advance(MESSAGE_HEADER_SIZE);

		let read_cstr = |src: &mut BytesMut| -> Result<String, ProtocolError> {
			let next_null = src.iter().position(|&b| b == 0).ok_or(ProtocolError::ParserError)?;
			let bytes = src[..next_null].to_owned();
			src.advance(bytes.len() + 1);
			Ok(String::from_utf8(bytes)?)
		};

		let message = match message_tag {
			b'P' => {
				let prepared_statement_name = read_cstr(src)?;
				let query = read_cstr(src)?;
				let num_params = src.get_i16();
				let _params: Vec<_> = (0..num_params).into_iter().map(|_| src.get_u32()).collect();

				ClientMessage::Parse(Parse {
					prepared_statement_name,
					query,
					parameter_types: Vec::new(),
				})
			}
			b'D' => {
				let target_type = src.get_u8();
				let name = read_cstr(src)?;

				ClientMessage::Describe(match target_type {
					b'P' => Describe::Portal(name),
					b'S' => Describe::PreparedStatement(name),
					_ => return Err(ProtocolError::ParserError),
				})
			}
			b'S' => ClientMessage::Sync,
			b'B' => {
				let portal = read_cstr(src)?;
				let prepared_statement_name = read_cstr(src)?;

				let num_param_format_codes = src.get_i16();
				for _ in 0..num_param_format_codes {
					let _format_code = src.get_i16();
				}

				let num_params = src.get_i16();
				for _ in 0..num_params {
					let param_len = src.get_i32() as usize;
					let _bytes = &src[0..param_len];
					src.advance(param_len);
				}

				let result_format = match src.get_i16() {
					0 => BindFormat::All(FormatCode::Text),
					1 => BindFormat::All(src.get_i16().try_into()?),
					n => {
						let mut result_format_codes = Vec::new();
						for _ in 0..n {
							result_format_codes.push(src.get_i16().try_into()?);
						}
						BindFormat::PerColumn(result_format_codes)
					}
				};

				ClientMessage::Bind(Bind {
					portal,
					prepared_statement_name,
					result_format,
				})
			}
			b'E' => {
				let portal = read_cstr(src)?;
				let max_rows = match src.get_i32() {
					0 => None,
					other => Some(other),
				};

				ClientMessage::Execute(Execute { portal, max_rows })
			}
			b'Q' => {
				let query = read_cstr(src)?;
				ClientMessage::Query(query)
			}
			b'X' => ClientMessage::Terminate,
			other => return Err(ProtocolError::InvalidMessageType(other)),
		};

		Ok(Some(message))
	}
}

impl<T: BackendMessage> Encoder<T> for ConnectionCodec {
	type Error = ProtocolError;

	fn encode(&mut self, item: T, dst: &mut BytesMut) -> Result<(), Self::Error> {
		let mut body = BytesMut::new();
		item.encode(&mut body);

		dst.put_u8(T::TAG);
		dst.put_i32((body.len() + 4) as i32);
		dst.put_slice(&body);
		Ok(())
	}
}
