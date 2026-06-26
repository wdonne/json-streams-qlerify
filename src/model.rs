use imbl::{HashMap, Vector};
use imbl_util::hashmap;
use imbl_util::vector::push_back;
use immutable_json::object::Object;
use iter_util::between::BetweenExt;
use iter_util::fold_result::FoldResultExt;
use log::warn;
use std::collections::HashSet;

use crate::error::ToolError;

#[derive(Clone, Debug)]
pub(crate) struct Aggregate {
    pub(crate) commands: HashMap<String, SchemaObject>,
    pub(crate) entity: String,
}

#[derive(Clone, Debug)]
pub(crate) struct App {
    pub(crate) aggregates: HashMap<String, Aggregate>,
    pub(crate) entities: HashMap<String, SchemaObject>,
    pub(crate) name: String,
    pub(crate) value_objects: HashMap<String, SchemaObject>,
    pub(crate) version: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Related {
    Entity(String),
    ValueObject(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    pub(crate) data_type: String,
    pub(crate) description: Option<String>,
    pub(crate) name: String,
    pub(crate) related: Option<Related>,
    pub(crate) required: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SchemaObject {
    pub(crate) description: Option<String>,
    pub(crate) fields: Vector<Field>,
    pub(crate) name: String,
}

struct SchemaRef {
    kind: String,
    name: String,
}

fn create_aggregate(
    event: &Object,
    reference: &str,
    model: &Object,
) -> Result<Aggregate, ToolError> {
    Ok(Aggregate {
        commands: get_command(event, model)?,
        entity: name(reference)?,
    })
}

fn command_description(event: &Object) -> Option<String> {
    event.get_array("acceptanceCriteria").map(|a| {
        String::from_iter(
            a.iter()
                .filter_map(|v| v.as_string())
                .between("\n".to_string()),
        )
    })
}

fn create_command(event: &Object, model: &Object) -> Result<SchemaObject, ToolError> {
    let reference = get_command_ref(event)?;
    let schema = get_schema(model, &reference)?;

    Ok(SchemaObject {
        description: command_description(event),
        fields: fields(&schema)?,
        name: name(&reference)?,
    })
}

fn create_field(object: &Object, required: &HashSet<String>) -> Result<Field, ToolError> {
    let name = object.get_string("name").ok_or(ToolError::FieldNoName)?;
    let (data_type, related) = type_info(object);

    Ok(Field {
        name: name.clone(),
        data_type,
        required: required.contains(&name),
        description: object.get_string("description"),
        related,
    })
}

fn type_info(field: &Object) -> (String, Option<Related>) {
    match (related(field), field.get_bool("array").unwrap_or(false)) {
        (Some(r), true) => ("array".to_string(), Some(r)),
        (Some(r), false) => ("object".to_string(), Some(r)),
        _ => (
            field
                .get_string("data_type")
                .unwrap_or_else(|| "string".to_string()),
            None,
        ),
    }
}

pub(crate) fn extract(model: &Object) -> Result<App, ToolError> {
    let aggregates = get_aggregates(model)?;

    model.get_string("boundedContext").map_or_else(
        || Err(ToolError::NoBoundedContext),
        |context| {
            Ok(App {
                name: context,
                version: get_version(model),
                aggregates: aggregates.clone(),
                entities: get_schema_objects(model, "/schemas/entities")?,
                value_objects: get_schema_objects(model, "/schemas/valueObjects")?,
            })
        },
    )
}

fn fields(schema: &Object) -> Result<Vector<Field>, ToolError> {
    let required = required(schema);

    schema.get_array("fields").map_or_else(
        || Ok(Vector::new()),
        |a| {
            a.iter()
                .filter_map(|v| v.as_object())
                .map(|o| create_field(&o, &required))
                .fold_result(Vector::new(), |v, f| push_back(&v, f))
        },
    )
}

fn get_aggregates(model: &Object) -> Result<HashMap<String, Aggregate>, ToolError> {
    model.get_object("domainEvents").map_or_else(
        || Err(ToolError::NoDomainEvents),
        |events| {
            events
                .iter()
                .filter_map(|(_, v)| v.as_object())
                .filter_map(|event| {
                    event
                        .get_string_p("/aggregateRoot/$ref")
                        .map(|r| (event, r))
                })
                .map(|(event, reference)| create_aggregate(&event, &reference, model))
                .fold_result(HashMap::new(), |m, a| {
                    m.update_with(a.entity.clone(), a, merge)
                })
        },
    )
}

fn get_command(event: &Object, model: &Object) -> Result<HashMap<String, SchemaObject>, ToolError> {
    let command = create_command(event, model)?;

    Ok(hashmap::insert(&HashMap::new(), command.name.clone(), command).0)
}

fn get_command_ref(event: &Object) -> Result<String, ToolError> {
    event.get_string_p("/command/$ref").ok_or_else(|| {
        ToolError::NoCommand(event.get_string("event").unwrap_or("unknown".to_string()))
    })
}

fn get_object(model: &Object, reference: &str) -> Option<Object> {
    pointer(reference).and_then(|p| model.get_object_p(&p))
}

fn get_schema(model: &Object, reference: &str) -> Result<Object, ToolError> {
    get_object(model, reference).ok_or_else(|| ToolError::NoSchema(reference.to_string()))
}

fn get_schema_objects(
    model: &Object,
    pointer: &str,
) -> Result<HashMap<String, SchemaObject>, ToolError> {
    model
        .get_object_p(pointer)
        .unwrap_or_default()
        .iter()
        .filter_map(|(k, v)| v.as_object().map(|o| (k, o)))
        .map(|(k, v)| schema_object(&k, &v))
        .fold_result(HashMap::new(), |m, s| {
            hashmap::insert(&m, s.name.clone(), s).0
        })
}

fn get_version(model: &Object) -> String {
    model
        .get_integer("version")
        .map(|i| i.to_string())
        .unwrap_or("unknown".to_string())
}

fn has_field(fields: &Vector<Field>, field: &Field) -> bool {
    fields.iter().any(|f| f == field)
}

fn merge(old_value: Aggregate, new_value: Aggregate) -> Aggregate {
    Aggregate {
        commands: hashmap::merge(&new_value.commands, &old_value.commands),
        entity: old_value.entity,
    }
}

fn name(pointer: &str) -> Result<String, ToolError> {
    pointer
        .split("/")
        .last()
        .ok_or_else(|| ToolError::InvalidPointer(pointer.to_string()))
        .map(|s| s.to_string())
}

fn pointer(reference: &str) -> Option<String> {
    if reference.starts_with("#/") {
        reference.strip_prefix("#").map(|s| s.to_string())
    } else {
        None
    }
}

fn related(field: &Object) -> Option<Related> {
    field
        .get_string_p("/relatedEntity/$ref")
        .and_then(|r| schema_ref(&r))
        .and_then(|r| match r.kind.as_str() {
            "entities" => Some(Related::Entity(r.name)),
            "valueObjects" => Some(Related::ValueObject(r.name)),
            _ => None,
        })
}

fn required(schema: &Object) -> HashSet<String> {
    schema.get_array("required").map_or_else(HashSet::new, |a| {
        a.iter().filter_map(|v| v.as_string()).collect()
    })
}

fn schema_object(name: &str, object: &Object) -> Result<SchemaObject, ToolError> {
    Ok(SchemaObject {
        description: object.get_string("description"),
        fields: fields(object)?,
        name: name.to_string(),
    })
}

fn schema_ref(reference: &str) -> Option<SchemaRef> {
    let v: Vec<&str> = reference.split('/').collect();

    if v.len() == 4 && v[0] == "#" && v[1] == "schemas" {
        Some(SchemaRef {
            kind: v[2].to_string(),
            name: v[3].to_string(),
        })
    } else {
        None
    }
}

pub(crate) fn verify_fields(app: &App) {
    app.aggregates
        .values()
        .for_each(|a| verify_fields_aggregate(app, a));
}

fn verify_fields_aggregate(app: &App, aggregate: &Aggregate) {
    aggregate
        .commands
        .values()
        .for_each(|c| verify_fields_command(app, aggregate, c));
}

fn verify_fields_command(app: &App, aggregate: &Aggregate, command: &SchemaObject) {
    match app.entities.get(&aggregate.entity) {
        Some(e) => {
           command.fields.iter().for_each(|f| {
                if !has_field(&e.fields, f) {
                    warn!("The command {0} of aggregate {1} has the field {2}, which does not exists in the aggregate",
                        command.name, aggregate.entity, f.name);
                }
           })},
        None => warn!("The entity for aggregate {0} is not defined", aggregate.entity),
    }
}
