use ironoxide::{
    prelude::*,
    user::{UserCreateOpts, UserResult},
    InitAndRotationCheck,
};
use lazy_static::*;
use std::{convert::TryInto, default::Default};
use uuid::Uuid;

pub const USER_PASSWORD: &str = "foo";

#[derive(serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    project_id: usize,
    segment_id: String,
    identity_assertion_key_id: usize,
}

lazy_static! {
    pub static ref ENV: String = match std::env::var("IRONCORE_ENV") {
        Ok(url) => match url.to_lowercase().as_ref() {
            "dev" => "-dev",
            "stage" => "-stage",
            "prod" => "-prod",
            _ => "",
        },
        _ => "-prod",
    }
    .to_string();
    static ref KEYPATH: (String, std::path::PathBuf) = {
        let mut path = std::env::current_dir().unwrap();
        let filename = format!("iak{}.pem", *ENV);
        path.push("tests");
        path.push("testkeys");
        path.push(filename.clone());
        (filename, path)
    };
    static ref IRONCORE_CONFIG_PATH: (String, std::path::PathBuf) = {
        let mut path = std::env::current_dir().unwrap();
        let filename = format!("ironcore-config{}.json", *ENV);
        path.push("tests");
        path.push("testkeys");
        path.push(filename.clone());
        (filename, path)
    };
    static ref CONFIG: Config = {
        use std::{fs::File, io::Read};
        let mut file = File::open(IRONCORE_CONFIG_PATH.1.clone()).unwrap_or_else(|err| {
            panic!(
                "Failed to open config file ({}) with error '{}'",
                IRONCORE_CONFIG_PATH.0, err
            )
        });
        let mut json_config = String::new();
        file.read_to_string(&mut json_config).unwrap_or_else(|err| {
            panic!(
                "Failed to read config file ({}) with error '{}'",
                IRONCORE_CONFIG_PATH.0, err
            )
        });
        serde_json::from_str(&json_config).unwrap_or_else(|err| {
            panic!(
                "Failed to deserialize config file ({}) with error '{}'",
                IRONCORE_CONFIG_PATH.0, err
            )
        })
    };
}

pub fn gen_jwt(account_id: Option<&str>) -> (String, String) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let start = SystemTime::now();
    let iat_seconds = start
        .duration_since(UNIX_EPOCH)
        .expect("Time before epoch? Something's wrong.")
        .as_secs();
    let jwt_header = json!({});
    let default_account_id = Uuid::new_v4().to_string();
    let sub = account_id.unwrap_or(&default_account_id);
    let jwt_payload = json!({
        "pid" : CONFIG.project_id,
        "sid" : CONFIG.segment_id,
        "kid" : CONFIG.identity_assertion_key_id,
        "iat" : iat_seconds,
        "exp" : iat_seconds + 120,
        "sub" : sub
    });
    let jwt = frank_jwt::encode(
        jwt_header,
        &KEYPATH.1.to_path_buf(),
        &jwt_payload,
        frank_jwt::Algorithm::ES256,
    )
    .expect(
        &format!("Error with {}: You don't appear to have the proper service private key to sign the test JWT.", KEYPATH.0)
    );
    (jwt, sub.to_string())
}

// This function is similar to init_sdk_get_user, but is more streamlined
// by not discarding InitAndRotationCheck, not calling user_verify for each
// user_create, and not manually unwrapping/re-wrapping the DeviceContext.
// The intent is that this will be used in most of the tests, as those extra
// verifications are not the goal of most tests. It also returns a result to give
// nice error handling with `?` in the tests.
pub fn initialize_sdk() -> Result<IronOxide, IronOxideErr> {
    let account_id: UserId = create_id_all_classes("").try_into()?;
    IronOxide::user_create(
        &gen_jwt(Some(account_id.id())).0,
        USER_PASSWORD,
        &UserCreateOpts::new(false),
    )?;
    let device = IronOxide::generate_new_device(
        &gen_jwt(Some(account_id.id())).0,
        USER_PASSWORD,
        &Default::default(),
    )?;
    ironoxide::initialize(&device)
}

pub fn init_sdk_get_user() -> (UserId, IronOxide) {
    let (u, init_check) = init_sdk_get_init_result(false);
    (u, init_check.discard_check())
}

pub fn init_sdk_get_init_result(user_needs_rotation: bool) -> (UserId, InitAndRotationCheck) {
    let account_id: UserId = create_id_all_classes("").try_into().unwrap();
    IronOxide::user_create(
        &gen_jwt(Some(account_id.id())).0,
        USER_PASSWORD,
        &UserCreateOpts::new(user_needs_rotation),
    )
    .unwrap();

    let verify_resp = IronOxide::user_verify(&gen_jwt(Some(account_id.id())).0)
        .unwrap()
        .unwrap();
    assert_eq!(&account_id, verify_resp.account_id());

    let device = IronOxide::generate_new_device(
        &gen_jwt(Some(account_id.id())).0,
        USER_PASSWORD,
        &Default::default(),
    )
    .unwrap();

    //Manually unwrap all of these types and rewrap them just to prove that we can construct the DeviceContext
    //from its raw parts as part of the exposed SDK

    let users_account_id = device.account_id().id();
    let users_segment_id = device.segment_id();
    let users_device_private_key_bytes = &device.device_private_key().as_bytes()[..];
    let users_signing_keys_bytes = &device.signing_private_key().as_bytes()[..];

    let device_init = DeviceContext::new(
        users_account_id.try_into().unwrap(),
        users_segment_id,
        users_device_private_key_bytes.try_into().unwrap(),
        users_signing_keys_bytes.try_into().unwrap(),
    );
    (
        account_id,
        ironoxide::initialize_check_rotation(&device_init).unwrap(),
    )
}

// this warns that it's unused because it's only used in other files,
// so this is suppressed to avoid false positive.
#[allow(dead_code)]
pub fn create_second_user() -> UserResult {
    let (jwt, _) = gen_jwt(Some(&create_id_all_classes("")));
    let create_result = IronOxide::user_create(&jwt, USER_PASSWORD, &Default::default());
    assert!(create_result.is_ok());

    let verify_result = IronOxide::user_verify(&jwt);
    assert!(verify_result.is_ok());
    verify_result.unwrap().unwrap()
}

pub fn create_id_all_classes(prefix: &str) -> String {
    format!(
        "{}{}{}",
        prefix,
        "abcABC012_.$#|@/:;=+'-",
        Uuid::new_v4().to_string()
    )
}

#[allow(dead_code)]
// Use this test to print out a JWT and UUID if you need it
fn non_test_print_jwt() {
    dbg!(gen_jwt(None));
}
