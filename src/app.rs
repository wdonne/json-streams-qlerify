use crate::{
    AppArgs,
    common::{get_file, path, read_json, to_yaml, write_yaml},
    error::ToolError,
    model::{Aggregate, App, Field, Related, SchemaObject, extract, verify_fields},
};
use generic_builders::immutable::Builder;
use imbl::Vector;
use imbl_util::vector::{append, push_back};
use immutable_json::{api::Value, array::Array, object::Object};
use iter_util::fold_result::FoldResultExt;
use std::io::Write;
use std::path::Path;

const REDUCER: &str = ".state + (.command | del(._command) | del(._languages) | del(._seq))";

fn add_prefix(field: &Field, prefix: &str) -> Field {
    Field {
        name: prefix.to_string() + "." + &field.name,
        ..field.clone()
    }
}

fn convert_data_type(data_type: &str) -> &'static str {
    match data_type {
        "array" => "array",
        "boolean" => "bool",
        "number" => "decimal",
        "object" => "object",
        "string" => "string",
        _ => "unknown",
    }
}

fn create_aggregate(
    app: &App,
    aggregate: &Aggregate,
    directory: &Path,
    mock_mode: bool,
) -> Result<String, ToolError> {
    let name = name(aggregate);
    let commands = create_commands(app, aggregate, directory, mock_mode)?;
    let generated = Builder::new(Object::new())
        .update(|o| o.add_string("type", "aggregate"))
        .update(|o| o.add_string("name", &name))
        .update(|o| {
            o.add_string(
                "aggregateType",
                &(app.name.clone() + "-" + &name).to_lowercase(),
            )
        })
        .update(|o| o.add_object("commands", &commands))
        .update_if_some(
            |_| {
                app.entities
                    .get(&aggregate.entity)
                    .and_then(|e| e.description.as_ref())
            },
            |o, d| o.add_string("description", d),
        )
        .build();
    let filename = name.clone() + "/" + &name + ".yaml";

    write_yaml(
        &Value::Object(generated),
        &mut get_file(directory, &filename)?,
    )?;

    Ok(filename)
}

fn create_aggregates(app: &App, directory: &Path, mock_mode: bool) -> Result<Array, ToolError> {
    app.aggregates
        .iter()
        .map(|(_, v)| create_aggregate(app, v, directory, mock_mode))
        .fold_result(Array::new(), |array, filename| array.add_string(&filename))
}

fn create_command(
    app: &App,
    command: &SchemaObject,
    directory: &Path,
    mock_mode: bool,
) -> Result<(String, Object), ToolError> {
    let reducer_filename = "reducers/".to_string() + &command.name + ".jq";
    let validator_filename = "validators/".to_string() + &command.name + ".yaml";

    if mock_mode || !path(directory, &reducer_filename).exists() {
        get_file(directory, &reducer_filename)?.write_all(REDUCER.as_bytes())?;
    }

    if mock_mode || !path(directory, &validator_filename).exists() {
        get_file(directory, &validator_filename)?
            .write_all(create_validator(command, app)?.as_bytes())?;
    }

    Ok((
        command.name.clone(),
        Builder::new(Object::new())
            .update(|o| o.add_string("reducer", &reducer_filename))
            .update(|o| o.add_string("validator", &validator_filename))
            .update_if_some(
                |_| command.description.clone(),
                |o, s| o.add_string("description", s),
            )
            .build(),
    ))
}

fn create_commands(
    app: &App,
    aggregate: &Aggregate,
    directory: &Path,
    mock_mode: bool,
) -> Result<Object, ToolError> {
    aggregate
        .commands
        .iter()
        .map(|(_, v)| create_command(app, v, directory, mock_mode))
        .fold_result(Object::new(), |object, (name, command)| {
            object.add_object(&name, &command)
        })
}

fn create_conditions(app: &App, command: &SchemaObject) -> Array {
    command
        .fields
        .iter()
        .flat_map(|f| create_field_conditions(app, f))
        .fold(Array::new(), |a, f| a.add_object(&f))
}

fn create_field_conditions(app: &App, field: &Field) -> Vector<Object> {
    Builder::new(Vector::new())
        .update(|v| push_back(&v, type_condition(field)))
        .update_if(
            |_| field.required,
            |v| push_back(&v, required_condition(field)),
        )
        .update_if(
            |_| field.related.is_some() && field.data_type != "array",
            |v| append(&v, &create_sub_field_conditions(app, field)),
        )
        .build()
}

fn create_sub_field_conditions(app: &App, field: &Field) -> Vector<Object> {
    match field.related.as_ref() {
        Some(Related::Entity(e)) => app.entities.get(e),
        Some(Related::ValueObject(e)) => app.value_objects.get(e),
        None => None,
    }
    .map(|e| {
        e.fields
            .iter()
            .flat_map(|f| create_field_conditions(app, &add_prefix(f, &field.name)))
            .collect()
    })
    .unwrap_or_default()
}

fn create_validator(command: &SchemaObject, app: &App) -> Result<String, ToolError> {
    to_yaml(&Value::Object(
        Object::new().add_array("conditions", &create_conditions(app, command)),
    ))
}

pub(crate) fn generate(args: &AppArgs) -> Result<(), ToolError> {
    let app = extract(&read_json(&args.file)?)?;

    if args.mock_mode {
        verify_fields(&app);
    }

    let generated = Object::new()
        .add_string("application", &app.name.to_lowercase())
        .add_string("version", &app.version)
        .add_array(
            "parts",
            &create_aggregates(&app, &args.directory, args.mock_mode)?
                .add_string("extra-parts.yaml"),
        );

    if !path(&args.directory, "extra-parts.yaml").exists() {
        write_yaml(
            &Value::Array(Array::new()),
            &mut get_file(&args.directory, "extra-parts.yaml")?,
        )?;
    }

    write_yaml(
        &Value::Object(generated),
        &mut get_file(&args.directory, "application.yaml")?,
    )?;

    Ok(())
}

fn name(aggregate: &Aggregate) -> String {
    aggregate.entity.to_lowercase()
}

fn required_condition(field: &Field) -> Object {
    Object::new().add_object(
        &field.name,
        &Object::new()
            .add_bool("$exists", true)
            .add_string("$code", "REQUIRED"),
    )
}

fn type_condition(field: &Field) -> Object {
    let converted_type = convert_data_type(&field.data_type);

    Object::new().add_object(
        &field.name,
        &Object::new()
            .add_string("$type", converted_type)
            .add_string("$code", &converted_type.to_uppercase()),
    )
}
