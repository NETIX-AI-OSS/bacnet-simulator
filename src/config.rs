pub mod minimal;

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::simulation::profiles::ProfileSpec;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BuildingConfig {
    pub name: String,
    /// Physical location of the building (informational only; not used at runtime).
    #[serde(default)]
    pub location: Option<String>,
    /// IANA timezone identifier (informational only; SeasonalityEngine uses system-local time).
    #[serde(default)]
    pub timezone: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TimeValue {
    pub time: String,
    pub value: f64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WeeklySchedule {
    pub weekday_occupancy: Vec<TimeValue>,
    pub weekend_occupancy: Vec<TimeValue>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SeasonalityConfig {
    pub weekly_schedule: WeeklySchedule,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IdPolicy {
    pub device_id_base: u32,
    pub per_template_block: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TemplatePointSpec {
    pub label: String,
    pub object_type: String,
    #[serde(default)]
    pub units: Option<String>,
    pub profile: ProfileSpec,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AssetTemplate {
    /// Human-readable description of this template (informational only; not propagated to BACnet properties).
    #[serde(default)]
    pub description: String,
    pub points: Vec<TemplatePointSpec>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AssetInstanceSpec {
    pub template: String,
    pub name_prefix: String,
    /// Zone label for this instance block (informational only; not propagated to DeviceSpec).
    #[serde(default)]
    pub zone: Option<String>,
    pub count: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SimulatorConfig {
    pub building: BuildingConfig,
    pub seasonality: SeasonalityConfig,
    pub id_policy: IdPolicy,
    pub templates: HashMap<String, AssetTemplate>,
    pub instances: Vec<AssetInstanceSpec>,
}

#[derive(Debug, Clone)]
pub struct PointSpec {
    pub label: String,
    pub object_type: String,
    pub units: Option<String>,
    pub instance: u32,
    pub profile: ProfileSpec,
}

#[derive(Debug, Clone)]
pub struct DeviceSpec {
    pub device_id: u32,
    pub name: String,
    pub points: Vec<PointSpec>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    UnknownTemplate(String),
    CountExceedsBlock {
        template: String,
        count: u32,
        block: u32,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "io error: {}", e),
            ConfigError::Yaml(e) => write!(f, "yaml error: {}", e),
            ConfigError::UnknownTemplate(t) => write!(f, "unknown template: {}", t),
            ConfigError::CountExceedsBlock {
                template,
                count,
                block,
            } => write!(
                f,
                "instance count {} for template '{}' exceeds per_template_block {}",
                count, template, block
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Bundled sample configuration (same content as the repo `config.yaml`).
pub const DEFAULT_CONFIG: &str = include_str!("../config.yaml");

impl SimulatorConfig {
    /// Write the bundled sample config to `path` when it does not already exist.
    pub fn ensure_config_file(path: &Path) -> Result<bool, ConfigError> {
        if path.exists() {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
            }
        }
        std::fs::write(path, DEFAULT_CONFIG).map_err(ConfigError::Io)?;
        Ok(true)
    }

    pub fn load_from_file(path: &str) -> Result<Self, ConfigError> {
        let f = std::fs::File::open(path).map_err(ConfigError::Io)?;
        let config: SimulatorConfig = serde_yaml::from_reader(f).map_err(ConfigError::Yaml)?;
        Ok(config)
    }

    pub fn load_default_embedded() -> Result<Self, ConfigError> {
        serde_yaml::from_str(DEFAULT_CONFIG).map_err(ConfigError::Yaml)
    }

    pub fn write_config(path: &Path, config: &SimulatorConfig) -> Result<(), ConfigError> {
        let yaml = serde_yaml::to_string(config).map_err(ConfigError::Yaml)?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
            }
        }
        std::fs::write(path, yaml).map_err(ConfigError::Io)
    }

    pub fn write_default_config(path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
            }
        }
        std::fs::write(path, DEFAULT_CONFIG).map_err(ConfigError::Io)
    }

    pub fn expand(&self) -> Result<Vec<DeviceSpec>, ConfigError> {
        let mut devices = Vec::new();
        let block = self.id_policy.per_template_block.max(1);

        for (block_idx, inst) in self.instances.iter().enumerate() {
            let template = self
                .templates
                .get(&inst.template)
                .ok_or_else(|| ConfigError::UnknownTemplate(inst.template.clone()))?;

            if inst.count > block {
                return Err(ConfigError::CountExceedsBlock {
                    template: inst.template.clone(),
                    count: inst.count,
                    block,
                });
            }

            let block_start = self.id_policy.device_id_base + (block_idx as u32 + 1) * block;

            for n in 0..inst.count {
                let device_id = block_start + n;
                let name = format!("{}-{:03}", inst.name_prefix, n + 1);

                let mut instance_counters: HashMap<String, u32> = HashMap::new();
                let mut points = Vec::with_capacity(template.points.len());
                for tp in &template.points {
                    let counter = instance_counters.entry(tp.object_type.clone()).or_insert(0);
                    *counter += 1;
                    points.push(PointSpec {
                        label: tp.label.clone(),
                        object_type: tp.object_type.clone(),
                        units: tp.units.clone(),
                        instance: *counter,
                        profile: tp.profile.clone(),
                    });
                }

                devices.push(DeviceSpec {
                    device_id,
                    name,
                    points,
                });
            }
        }

        Ok(devices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> SimulatorConfig {
        let mut templates = HashMap::new();
        templates.insert(
            "tpl_a".to_string(),
            AssetTemplate {
                description: "test".into(),
                points: vec![
                    TemplatePointSpec {
                        label: "p1".into(),
                        object_type: "analog_input".into(),
                        units: None,
                        profile: ProfileSpec::Constant { value: 1.0 },
                    },
                    TemplatePointSpec {
                        label: "p2".into(),
                        object_type: "analog_input".into(),
                        units: None,
                        profile: ProfileSpec::Constant { value: 2.0 },
                    },
                    TemplatePointSpec {
                        label: "p3".into(),
                        object_type: "binary_input".into(),
                        units: None,
                        profile: ProfileSpec::ConstantBool { value: true },
                    },
                ],
            },
        );
        SimulatorConfig {
            building: BuildingConfig {
                name: "T".into(),
                location: None,
                timezone: None,
            },
            seasonality: SeasonalityConfig {
                weekly_schedule: WeeklySchedule {
                    weekday_occupancy: vec![],
                    weekend_occupancy: vec![],
                },
            },
            id_policy: IdPolicy {
                device_id_base: 10000,
                per_template_block: 100,
            },
            templates,
            instances: vec![AssetInstanceSpec {
                template: "tpl_a".into(),
                name_prefix: "A".into(),
                zone: None,
                count: 3,
            }],
        }
    }

    #[test]
    fn expansion_assigns_unique_device_ids() {
        let cfg = sample_config();
        let devs = cfg.expand().unwrap();
        assert_eq!(devs.len(), 3);
        let ids: Vec<u32> = devs.iter().map(|d| d.device_id).collect();
        assert_eq!(ids, vec![10100, 10101, 10102]);
        let mut s = std::collections::HashSet::new();
        for id in &ids {
            assert!(s.insert(*id));
        }
    }

    #[test]
    fn expansion_assigns_unique_object_ids_per_type_per_device() {
        let cfg = sample_config();
        let devs = cfg.expand().unwrap();
        let d0 = &devs[0];
        let mut seen = std::collections::HashSet::new();
        for p in &d0.points {
            assert!(seen.insert((p.object_type.clone(), p.instance)));
        }
        // Two analog_input points => instances 1, 2
        let ai: Vec<u32> = d0
            .points
            .iter()
            .filter(|p| p.object_type == "analog_input")
            .map(|p| p.instance)
            .collect();
        assert_eq!(ai, vec![1, 2]);
        // One binary_input => instance 1
        let bi: Vec<u32> = d0
            .points
            .iter()
            .filter(|p| p.object_type == "binary_input")
            .map(|p| p.instance)
            .collect();
        assert_eq!(bi, vec![1]);
    }

    #[test]
    fn expansion_errors_when_count_exceeds_block() {
        let mut cfg = sample_config();
        cfg.instances[0].count = 101;
        assert!(matches!(
            cfg.expand(),
            Err(ConfigError::CountExceedsBlock { .. })
        ));
    }

    #[test]
    fn expansion_errors_on_unknown_template() {
        let mut cfg = sample_config();
        cfg.instances[0].template = "nope".into();
        assert!(matches!(cfg.expand(), Err(ConfigError::UnknownTemplate(_))));
    }

    #[test]
    fn embedded_default_config_parses() {
        SimulatorConfig::load_default_embedded().expect("embedded default config must parse");
    }

    #[test]
    fn write_default_config_round_trip() {
        let dir = std::env::temp_dir().join(format!("bacnet-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        SimulatorConfig::write_default_config(&path).unwrap();
        let loaded = SimulatorConfig::load_from_file(path.to_str().unwrap()).unwrap();
        assert!(!loaded.instances.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_config_file_writes_only_when_missing() {
        let dir = std::env::temp_dir().join(format!(
            "bacnet-sim-config-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");

        assert!(SimulatorConfig::ensure_config_file(&path).unwrap());
        assert!(path.is_file());
        assert!(!SimulatorConfig::ensure_config_file(&path).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_config_expands_to_target_size_and_unique_ids() {
        let cfg = SimulatorConfig::load_from_file("config.yaml").expect("load real config");
        let devs = cfg.expand().expect("expand real config");
        let total_points: usize = devs.iter().map(|d| d.points.len()).sum();
        assert!(
            devs.len() >= 200 && devs.len() <= 320,
            "device count out of expected band: {}",
            devs.len()
        );
        assert!(
            total_points >= 1400 && total_points <= 1600,
            "point count out of expected band: {}",
            total_points
        );

        let mut device_ids = std::collections::HashSet::new();
        for d in &devs {
            assert!(
                device_ids.insert(d.device_id),
                "duplicate device_id {}",
                d.device_id
            );
            let mut tuples = std::collections::HashSet::new();
            for p in &d.points {
                assert!(
                    tuples.insert((p.object_type.clone(), p.instance)),
                    "duplicate (type,instance) in device {}",
                    d.device_id
                );
            }
        }
    }
}
