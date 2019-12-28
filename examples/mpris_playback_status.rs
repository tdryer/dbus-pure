// Connects to the session bus, enumerates all media players that implement MPRIS, and prints their playback status.

fn main() -> Result<(), Error> {
	let connection =
		dbus_pure::conn::Connection::new(
			dbus_pure::conn::BusPath::Session,
			dbus_pure::conn::SaslAuthType::Uid,
		)?;
	let mut client = dbus_pure::client::Client::new(connection)?;

	// List all names by calling the `org.freedesktop.DBus.ListNames` method
	// on the `/org/freedesktop/DBus` object at the destination `org.freedesktop.DBus`.
	let names =
		client.method_call(
			"org.freedesktop.DBus".to_owned(),
			dbus_pure::types::ObjectPath("/org/freedesktop/DBus".to_owned()),
			"org.freedesktop.DBus".to_owned(),
			"ListNames".to_owned(),
			None,
		)?
		.ok_or(None)
		.and_then(|body| body.into_array(&dbus_pure::types::Signature::String).map_err(Some))
		.map_err(|body| format!("ListNames response failed with {:#?}", body))?
		.into_iter()
		.map(|element| element.into_string().unwrap());

	// MPRIS media players have names that start with "org.mpris.MediaPlayer2."
	let media_player_names = names.filter(|object_name| object_name.starts_with("org.mpris.MediaPlayer2."));

	for media_player_name in media_player_names {
		println!("Found media player {}", media_player_name);

		// Get the playback status of the media player by gettings its `PlaybackStatus` property.
		//
		// The property is exposed by the object at path `/org/mpris/MediaPlayer2`
		// on the `org.mpris.MediaPlayer2.Player` interface.
		//
		// Properties in general are accessed by calling the `org.freedesktop.DBus.Properties.Get` method
		// with two parameters - the interface name and the property name.
		let playback_status =
			client.method_call(
				media_player_name.clone(),
				dbus_pure::types::ObjectPath("/org/mpris/MediaPlayer2".to_owned()),
				"org.freedesktop.DBus.Properties".to_owned(),
				"Get".to_owned(),
				Some(&dbus_pure::types::Variant::Tuple {
					elements: vec![
						dbus_pure::types::Variant::String("org.mpris.MediaPlayer2.Player".to_owned()),
						dbus_pure::types::Variant::String("PlaybackStatus".to_owned()),
					],
				}),
			)?
			.ok_or(None)
			.and_then(|body| body.into_variant().map_err(Some))
			.and_then(|body| body.into_string().map_err(Some))
			.map_err(|body| format!("GetPlaybackStatus response failed with {:#?}", body))?;

		println!("{} is {}", media_player_name, playback_status);
	}

	Ok(())
}

struct Error(Box<dyn std::error::Error>);

impl<E> From<E> for Error where E: Into<Box<dyn std::error::Error>> {
	fn from(err: E) -> Self {
		Error(err.into())
	}
}

impl std::fmt::Debug for Error {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		writeln!(f, "{}", self.0)?;

		let mut source = self.0.source();
		while let Some(err) = source {
			writeln!(f, "caused by: {}", err)?;
			source = err.source();
		}

		Ok(())
	}
}