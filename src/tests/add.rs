// Copyright (c) 2019, Ben Boeckel
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without modification,
// are permitted provided that the following conditions are met:
//
//     * Redistributions of source code must retain the above copyright notice,
//       this list of conditions and the following disclaimer.
//     * Redistributions in binary form must reproduce the above copyright notice,
//       this list of conditions and the following disclaimer in the documentation
//       and/or other materials provided with the distribution.
//     * Neither the name of this project nor the names of its contributors
//       may be used to endorse or promote products derived from this software
//       without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE FOR
// ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
// ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::borrow::Cow;
use std::iter;

use crate::keytype::KeyPayload;
use crate::keytypes::encrypted::{Format, MasterKeyType, Payload};
use crate::keytypes::{Encrypted, User};
use crate::{Keyring, KeyringSerial, SpecialKeyring};

use super::utils::kernel::*;
use super::utils::keys::*;

#[test]
fn empty_key_type() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Process).unwrap();
    let err = keyring.add_key::<EmptyKey, _, _>("", ()).unwrap_err();
    assert_eq!(err, errno::Errno(libc::EINVAL));
}

#[test]
fn unsupported_key_type() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Process).unwrap();
    let err = keyring.add_key::<UnsupportedKey, _, _>("", ()).unwrap_err();
    assert_eq!(err, errno::Errno(libc::ENODEV));
}

#[test]
fn invalid_key_type() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Process).unwrap();
    let err = keyring.add_key::<InvalidKey, _, _>("", ()).unwrap_err();
    assert_eq!(err, errno::Errno(libc::EPERM));
}

#[test]
fn maxlen_key_type() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Process).unwrap();
    let err = keyring.add_key::<MaxLenKey, _, _>("", ()).unwrap_err();
    assert_eq!(err, errno::Errno(libc::ENODEV));
}

#[test]
fn overlong_key_type() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Process).unwrap();
    let err = keyring.add_key::<OverlongKey, _, _>("", ()).unwrap_err();
    assert_eq!(err, errno::Errno(libc::EINVAL));
}

#[test]
fn keyring_with_payload() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Process).unwrap();
    let err = keyring
        .add_key::<KeyringShadow, _, _>("", "payload")
        .unwrap_err();
    assert_eq!(err, errno::Errno(libc::EINVAL));
}

#[test]
fn max_user_description() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Process).unwrap();
    // Subtract one because the NUL is added in the kernel API.
    let maxdesc: String = iter::repeat('a').take(*PAGE_SIZE - 1).collect();
    let res = keyring.add_key::<User, _, _>(maxdesc.as_ref(), "payload".as_bytes());
    // If the user's quota is smaller than this, it's an error.
    if KEY_INFO.maxbytes < *PAGE_SIZE {
        assert_eq!(res.unwrap_err(), errno::Errno(libc::EDQUOT));
    } else {
        let key = res.unwrap();
        assert_eq!(key.description().unwrap().description, maxdesc);
        key.invalidate().unwrap();
    }
}

#[test]
fn overlong_user_description() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Process).unwrap();
    // On MIPS with < 3.19, there is a bug where this is allowed. 3.19 was released in Feb 2015,
    // so this is being ignored here.
    let toolarge: String = iter::repeat('a').take(*PAGE_SIZE).collect();
    let err = keyring
        .add_key::<User, _, _>(toolarge.as_ref(), "payload".as_bytes())
        .unwrap_err();
    assert_eq!(err, errno::Errno(libc::EINVAL));
}

#[test]
fn invalid_keyring() {
    // Yes, we're explicitly breaking the NonZeroI32 rules here. However, it is not passing
    // through any bits which care (e.g., `Option`), so this is purely to test that using an
    // invalid keyring ID gives back `EINVAL` as expected.
    let mut keyring = unsafe { Keyring::new(KeyringSerial::new_unchecked(0)) };
    let err = keyring
        .add_key::<User, _, _>("desc", "payload".as_bytes())
        .unwrap_err();
    assert_eq!(err, errno::Errno(libc::EINVAL));
}

#[test]
fn add_key_to_session() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Session).unwrap();
    let expected = "stuff".as_bytes();
    let mut key = keyring.add_key::<User, _, _>("wibble", expected).unwrap();
    let payload = key.read().unwrap();
    assert_eq!(payload, expected);
    let new_expected = "lizard".as_bytes();
    key.update(new_expected).unwrap();
    let new_payload = key.read().unwrap();
    assert_eq!(new_payload, new_expected);
    keyring.unlink_key(&key).unwrap();
}

#[test]
fn add_encrypted_key_to_session() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Session).unwrap();
    let master_key = keyring
        .add_key::<User, _, _>("foo", "bar".as_bytes())
        .unwrap();
    let new_payload = Payload::New {
        format: Some(Format::default()),
        keytype: MasterKeyType::User,
        description: Cow::Borrowed("foo"),
        keylen: 32,
    };

    let mut enc_key = keyring
        .add_key::<Encrypted, _, _>("baz", new_payload)
        .unwrap();

    // A normal payload update fails
    assert_eq!(
        enc_key.update("qux".as_bytes()),
        Err(errno::Errno(libc::EINVAL))
    );

    keyring.unlink_key(&enc_key).unwrap();
    keyring.unlink_key(&master_key).unwrap();
}

#[test]
#[should_panic(
    expected = "called `Result::unwrap()` on an `Err` value: Errno { code: 22, description: Some(\"Invalid argument\") }"
)]
fn load_encrypted_key_to_session() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Session).unwrap();
    let master_key = keyring
        .add_key::<User, _, _>("foo", "bar".as_bytes())
        .unwrap();
    let new_payload = Payload::New {
        format: Some(Format::default()),
        keytype: MasterKeyType::User,
        description: Cow::Borrowed("foo"),
        keylen: 32,
    };
    let enc_key = keyring
        .add_key::<Encrypted, _, _>("baz", new_payload)
        .unwrap();
    let buf = enc_key.read().unwrap();

    let load_payload = Payload::Load {
        blob: buf.clone(),
    };

    keyring.unlink_key(&enc_key).unwrap();

    // This should not panic but currently does due to the use
    // of ByteBuf when encoding the load payload.
    let load_key = keyring
        .add_key::<Encrypted, _, _>("qux", load_payload)
        .unwrap();

    keyring.unlink_key(&load_key).unwrap();
    keyring.unlink_key(&master_key).unwrap();
}

#[test]
fn update_encrypted_key_in_session() {
    let mut keyring = Keyring::attach_or_create(SpecialKeyring::Session).unwrap();
    let old_master_key = keyring
        .add_key::<User, _, _>("foo", "bar".as_bytes())
        .unwrap();
    let new_master_key = keyring
        .add_key::<User, _, _>("bar", "foo".as_bytes())
        .unwrap();
    let new_payload = Payload::New {
        format: Some(Format::default()),
        keytype: MasterKeyType::User,
        description: Cow::Borrowed("foo"),
        keylen: 32,
    };

    let mut enc_key = keyring
        .add_key::<Encrypted, _, _>("baz", new_payload)
        .unwrap();

    // A normal payload update fails
    assert_eq!(
        enc_key.update("qux".as_bytes()),
        Err(errno::Errno(libc::EINVAL))
    );

    let update_payload = Payload::Update {
        keytype: MasterKeyType::User,
        description: Cow::Borrowed("bar"),
    };
    enc_key.update(update_payload.payload()).unwrap();

    keyring.unlink_key(&enc_key).unwrap();
    keyring.unlink_key(&old_master_key).unwrap();
    keyring.unlink_key(&new_master_key).unwrap();
}
