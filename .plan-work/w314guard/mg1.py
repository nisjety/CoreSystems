import sys; sys.path.insert(0,sys.argv[1]); from edit import edit
edit('src/citation_anchors.rs',[
("                origin: citation.origin.clone(),\n            });","                origin: citation.origin.clone(),\n                passage: citation.passage.clone(),\n            });",1),
("            number: Some(number),\n            origin: \"web\".to_owned(),\n        }","            number: Some(number),\n            origin: \"web\".to_owned(),\n            passage: None,\n        }",1),
("            number: None,\n            origin: \"connector\".to_owned(),\n        };","            number: None,\n            origin: \"connector\".to_owned(),\n            passage: None,\n        };",1),
])
edit('src/connector_reads.rs',[("        end: None,\n        origin: \"connector\".to_owned(),\n    }\n}","        end: None,\n        origin: \"connector\".to_owned(),\n        passage: None,\n    }\n}",1)])
edit('src/tool_loop.rs',[
("                end: None,\n                origin: \"web\".to_owned(),\n            })","                end: None,\n                origin: \"web\".to_owned(),\n                passage: None,\n            })",1),
("                    end: None,\n                    origin: \"web\".to_owned(),\n                }],","                    end: None,\n                    origin: \"web\".to_owned(),\n                    passage: None,\n                }],",1),
("                content,\n                version,\n            } => {\n                assert_eq!(id, \"q3-rapport\");","                content,\n                version,\n                ..\n            } => {\n                assert_eq!(id, \"q3-rapport\");",1),
])
edit('src/artifacts.rs',[
("        content: content.to_owned(),\n        version,\n    }\n}","        content: content.to_owned(),\n        version,\n        provenance: None,\n    }\n}",1),
("        match artifact_event(\"a1\", ArtifactKind::Document, \"Rapport\", \"# Hei\", 2) {\n            ChatEvent::Artifact {\n                id,\n                kind,\n                title,\n                content,\n                version,\n            }","        match artifact_event(\"a1\", ArtifactKind::Document, \"Rapport\", \"# Hei\", 2) {\n            ChatEvent::Artifact {\n                id,\n                kind,\n                title,\n                content,\n                version,\n                ..\n            }",1),
])
edit('src/artifact_revisions.rs',[("            content: format!(\"text {version}\"),\n            version,\n        }","            content: format!(\"text {version}\"),\n            version,\n            provenance: None,\n        }",1)])
edit('src/result_validation.rs',[("            version: 3,\n            content: \"## Overview\nShared offices\".into(),\n        }];","            version: 3,\n            provenance: None,\n            content: \"## Overview\nShared offices\".into(),\n        }];",1)])
