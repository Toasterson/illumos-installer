use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt::{Display, Formatter};
use url::Url;
use uuid::Uuid;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Manifest {
    //Version of the manifest format/spec. The current value is 2.
    pub v: i32,

    //The unique identifier for a UUID. This is set by the IMGAPI server. See details below.
    pub uuid: Uuid,

    //The UUID of the owner of this image (the account that created it).
    pub owner: Uuid,

    //A short name for this image. Max 512 characters (though practical usage should be much shorter). No uniqueness guarantee.
    pub name: String,

    //A version string for this image. Max 128 characters. No uniqueness guarantee.
    pub version: String,

    //A short description of the image.
    pub description: Option<String>,

    //Homepage URL where users can find more information about the image.
    pub homepage: Option<Url>,

    //URL of the End User License Agreement (EULA) for the image.
    pub eula: Option<Url>,

    //Indicates if the image has an icon file. If not present, then no icon is present.
    pub icon: Option<bool>,

    //The current state of the image. One of 'active', 'unactivated', 'disabled', 'creating', 'failed'.
    pub state: ImageState,

    //An object with details on image creation failure. It only exists when state=='failed'.
    pub error: Option<Map<String, Value>>,

    //Indicates if this image is available for provisioning.
    pub disabled: bool,

    //Indicates if this image is publicly available.
    pub public: bool,

    //The date at which the image is activated. Set by the IMGAPI server.
    pub published_at: Option<DateTime<Utc>>,

    //The image type. One of "zone-dataset" for a ZFS dataset used to create a new SmartOS zone, "lx-dataset" for a Lx-brand image, "lxd" for a LXD image, "zvol" for a virtual machine image or "other" for image types that serve any other specific purpose.
    #[serde(rename = "type")]
    pub image_type: ImageType,

    //The OS family this image provides. One of "smartos", "windows", "linux", "bsd", "illumos" or "other".
    pub os: ImageOs,

    //The origin image UUID if this is an incremental image.
    pub origin: Option<Uuid>,

    //An array of objects describing the image files.
    pub files: Vec<Map<String, Value>>,

    //Access Control List. An array of account UUIDs given access to a private image. The field is only relevant to private images.
    pub acl: Option<Vec<Uuid>>,

    //A set of named requirements for provisioning a VM with this image
    pub requirements: Option<ImageRequirements>,

    //A list of users for which passwords should be generated for provisioning. This may only make sense for some images. Example: [{"name": "root"}, {"name": "admin"}]
    pub users: Option<Vec<ImageUsers>>,

    //A list of tags that can be used by operators for additional billing processing.
    pub billing_tags: Option<Vec<String>>,

    //An object that defines a collection of properties that is used by other APIs to evaluate where should customer VMs be placed.
    pub traits: Option<Vec<String>>,

    //An object of key/value pairs that allows clients to categorize images by any given criteria.
    pub tags: Option<IndexMap<String, String>>,

    //A boolean indicating whether to generate passwords for the users in the "users" field. If not present, the default value is true.
    pub generate_password: Option<bool>,

    //A list of inherited directories (other than the defaults for the brand).
    pub inherited_directories: Option<Vec<String>>,

    //Array of channel names to which this image belongs.
    pub channels: Option<Vec<String>>,

    #[serde(flatten)]
    pub vm_image_properties: Option<ImageVMProperties>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ImageState {
    Active,
    Unactivated,
    Disabled,
    Creating,
    Failed,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum ImageType {
    ZoneDataset,
    LxDataset,
    Lxd,
    Zvol,
    Other,
}

impl Display for ImageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageType::ZoneDataset => {
                write!(f, "zone-dataset")
            }
            ImageType::LxDataset => {
                write!(f, "lx-dataset")
            }
            ImageType::Lxd => {
                write!(f, "lxd")
            }
            ImageType::Zvol => {
                write!(f, "zvol")
            }
            ImageType::Other => {
                write!(f, "other")
            }
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum ImageOs {
    Smartos,
    Windows,
    Linux,
    Bsd,
    Illumos,
    Other,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageRequirements {
    //Defines the minimum number of network interfaces required by this image.
    pub networks: Option<Vec<RequirementNetworks>>,

    //Defines the brand that is required to provision with this image.
    pub brand: Option<String>,

    //Indicates that provisioning with this image requires that an SSH public key be provided.
    pub ssh_key: Option<bool>,

    //Minimum RAM (in MiB) required to provision this image.
    pub min_ram: Option<i64>,

    //Maximum RAM (in MiB) this image may be provisioned with.
    pub max_ram: Option<i64>,

    //Minimum platform requirement for provisioning with this image.
    pub min_platform: Option<IndexMap<String, String>>,

    //Maximum platform requirement for provisioning with this image.
    pub max_platform: Option<IndexMap<String, String>>,

    //Bootrom image to use with this image.
    pub bootrom: Option<ImageRequirementBootRom>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct RequirementNetworks {
    name: String,
    description: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum ImageRequirementBootRom {
    Bios,
    Uefi,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageUsers {
    name: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageVMProperties {
    //NIC driver used by this VM image.
    pub nic_driver: String,

    //Disk driver used by this VM image.
    pub disk_driver: String,

    //The QEMU CPU model to use for this VM image.
    pub cpu_type: String,

    //The size (in MiB) of this VM image's disk.
    pub image_size: u64,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageFile {
    //SHA-1 hex digest of the file content. Used for upload/download corruption checking.
    pub sha1: String,

    //Number of bytes. Maximum 20GiB. This maximum is meant to be a "you'll never hit it" cap, the purpose is to inform cache handling in IMGAPI servers.
    pub size: i64,

    //The type of file compression used by the file. One of 'bzip2', 'gzip', 'none'.
    pub compression: ImageFileCompression,

    //Optional. The ZFS internal unique identifier for this dataset's snapshot (available via zfs get guid SNAPSHOT, e.g. zfs get guid zones/f669428c-a939-11e2-a485-b790efc0f0c1@final). If available, this is used to ensure a common base snapshot for incremental images (via imgadm create -i) and VM migrations (via vmadm send/receive).
    pub dataset_guid: Option<String>,

    //Only included if ?inclAdminFields=true is passed to GetImage/ListImages. The IMGAPI storage type used to store this file.
    pub stor: Option<String>,

    //Optional. Docker digest of the file contents. Only used when manifest.type is 'docker'. This field gets set automatically by the AdminImportDockerImage call.
    pub digest: Option<String>,

    //Optional. Docker digest of the uncompressed file contents. Only used when manifest.type is 'docker'. This field gets set automatically by the AdminImportDockerImage call. Note that this field will be removed in a future version of IMGAPI.
    #[serde(rename = "uncompressedDigest")]
    pub uncompressed_digest: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum ImageFileCompression {
    Bzip2,
    Gzip,
    None,
}
