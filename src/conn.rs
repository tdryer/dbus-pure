/// A connection to a message bus.
pub struct Connection {
	reader: std::io::BufReader<std::os::unix::net::UnixStream>,
	read_buf: Vec<u8>,
	read_end: usize,
	writer: std::os::unix::net::UnixStream,
	write_buf: Vec<u8>,
	server_guid: Vec<u8>,
}

/// The path of a message bus.
#[derive(Clone, Copy, Debug)]
pub enum BusPath<'a> {
	/// The session bus. Its path will be determined from the `DBUS_SESSION_BUS_ADDRESS` environment variable.
	Session,

	/// A unix domain socket file at the specified filesystem path.
	UnixSocketFile(&'a std::path::Path),
}

/// The string to send for SASL EXTERNAL authentication with the message bus.
///
/// `Uid` is usually the type to use for local message buses.
#[derive(Clone, Copy, Debug)]
pub enum SaslAuthType<'a> {
	/// The user ID of the current thread will be used.
	Uid,

	/// The specified string will be used.
	Other(&'a str),
}

impl Connection {
	/// Opens a connection to the bus at the given path with the given authentication type.
	pub fn new(
		bus_path: BusPath<'_>,
		sasl_auth_type: SaslAuthType<'_>,
	) -> Result<Self, ConnectError> {
		use std::io::{BufRead, Write};

		let stream = match bus_path {
			BusPath::Session => {
				let session_bus_address = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").ok_or_else(|| ConnectError::SessionBusEnvVar(None))?;
				let bus_path: &std::ffi::OsStr = {
					let session_bus_address_bytes = std::os::unix::ffi::OsStrExt::as_bytes(&*session_bus_address);
					if session_bus_address_bytes.starts_with(b"unix:path=") {
						std::os::unix::ffi::OsStrExt::from_bytes(&session_bus_address_bytes["unix:path=".len()..])
					}
					else {
						return Err(ConnectError::SessionBusEnvVar(Some(session_bus_address)));
					}
				};
				let bus_path = std::path::Path::new(bus_path);
				let stream =
					std::os::unix::net::UnixStream::connect(bus_path)
					.map_err(|err| ConnectError::Connect { bus_path: bus_path.to_owned(), err, })?;
				stream
			},

			BusPath::UnixSocketFile(bus_path) => {
				let stream =
					std::os::unix::net::UnixStream::connect(bus_path)
					.map_err(|err| ConnectError::Connect { bus_path: bus_path.to_owned(), err, })?;
				stream
			},
		};

		let sasl_auth_id: std::borrow::Cow<'_, str> = match sasl_auth_type {
			SaslAuthType::Uid =>
				(unsafe { libc::getuid() })
				.to_string()
				.chars()
				.map(|c| format!("{:2x}", c as u32))
				.collect::<String>()
				.into(),

			SaslAuthType::Other(sasl_auth_id) => sasl_auth_id.into(),
		};

		let reader = stream.try_clone().map_err(ConnectError::Authenticate)?;
		let mut reader = std::io::BufReader::new(reader);
		let mut read_buf = vec![];

		let mut writer = stream;
		let write_buf = vec![];

		write!(writer, "\0AUTH EXTERNAL {}\r\n", sasl_auth_id).map_err(ConnectError::Authenticate)?;
		writer.flush().map_err(ConnectError::Authenticate)?;

		let _ = reader.read_until(b'\n', &mut read_buf).map_err(ConnectError::Authenticate)?;
		if read_buf.iter().rev().nth(1).copied() != Some(b'\r') {
			return Err(ConnectError::Authenticate(std::io::Error::new(std::io::ErrorKind::Other, "malformed response")));
		}

		let server_guid =
			if read_buf.starts_with(b"OK ") {
				&read_buf[b"OK ".len()..(b"OK ".len() + 32)]
			}
			else {
				return Err(ConnectError::Authenticate(std::io::Error::new(std::io::ErrorKind::Other, "malformed response")));
			};
		let server_guid = server_guid.to_owned();

		read_buf.clear();
		read_buf.resize(1, 0);

		writer.write_all(b"BEGIN\r\n").map_err(ConnectError::Authenticate)?;
		writer.flush().map_err(ConnectError::Authenticate)?;

		Ok(Connection {
			reader,
			read_buf,
			read_end: 0,
			writer,
			write_buf,
			server_guid,
		})
	}

	/// The GUID of the server.
	pub fn server_guid(&self) -> &[u8] {
		&self.server_guid
	}

	pub(crate) fn write_buf(&mut self) -> &mut Vec<u8> {
		&mut self.write_buf
	}

	pub(crate) fn flush(&mut self) -> std::io::Result<()> {
		use std::io::Write;

		self.writer.write_all(&self.write_buf)?;
		self.write_buf.clear();

		self.writer.flush()?;

		Ok(())
	}

	pub(crate) fn read_buf(&self) -> &[u8] {
		&self.read_buf[..self.read_end]
	}

	pub(crate) fn recv(&mut self) -> std::io::Result<()> {
		use std::io::Read;

		if self.read_end == self.read_buf.len() {
			self.read_buf.resize(self.read_buf.len() * 2, 0);
		}

		let read = self.reader.read(&mut self.read_buf[self.read_end..])?;
		if read == 0 {
			return Err(std::io::ErrorKind::UnexpectedEof.into());
		}

		self.read_end += read;

		Ok(())
	}

	pub(crate) fn consume(&mut self, consumed: usize) {
		self.read_buf.copy_within(consumed..self.read_end, 0);
		self.read_end -= consumed;
	}
}

/// An error from connecting to a message bus.
#[derive(Debug)]
pub enum ConnectError {
	Authenticate(std::io::Error),

	Connect {
		bus_path: std::path::PathBuf,
		err: std::io::Error,
	},

	SessionBusEnvVar(Option<std::ffi::OsString>),
}

impl std::fmt::Display for ConnectError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ConnectError::Authenticate(_) => f.write_str("could not authenticate with bus"),
			ConnectError::Connect { bus_path, err: _ } => write!(f, "could not connect to bus path {}", bus_path.display()),
			ConnectError::SessionBusEnvVar(None) => f.write_str("the DBUS_SESSION_BUS_ADDRESS env var is not set"),
			ConnectError::SessionBusEnvVar(Some(value)) => write!(f, "the DBUS_SESSION_BUS_ADDRESS env var is malformed: {:?}", value),
		}
	}
}

impl std::error::Error for ConnectError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		#[allow(clippy::match_same_arms)]
		match self {
			ConnectError::Authenticate(err) => Some(err),
			ConnectError::Connect { bus_path: _, err } => Some(err),
			ConnectError::SessionBusEnvVar(_) => None,
		}
	}
}