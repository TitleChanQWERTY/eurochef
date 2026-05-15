#[macro_use]
extern crate tracing;

mod edb;
mod filelist;

use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};

use anyhow::Context;
use eurochef_edb::binrw::BinReaderExt;
use clap::{Parser, Subcommand};
use clap_num::maybe_hex;
use eurochef_edb::edb::EdbFile;
use eurochef_edb::versions::Platform;
use tracing::metadata::LevelFilter;
use tracing_subscriber::{
    prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

#[derive(clap::ValueEnum, PartialEq, Debug, Clone)]
pub enum PlatformArg {
    Pc,
    Xb,
    Xbox,
    Xbox360,
    Ps2,
    Ps3,
    Gc,
    Gamecube,
    Wii,
    WiiU,
}

impl From<PlatformArg> for Platform {
    fn from(val: PlatformArg) -> Self {
        match val {
            PlatformArg::Pc => Platform::Pc,
            PlatformArg::Xbox | PlatformArg::Xb => Platform::Xbox,
            PlatformArg::Xbox360 => Platform::Xbox360,
            PlatformArg::Ps2 => Platform::Ps2,
            PlatformArg::Ps3 => Platform::Ps3,
            PlatformArg::Gamecube | PlatformArg::Gc => Platform::GameCube,
            PlatformArg::Wii => Platform::Wii,
            PlatformArg::WiiU => Platform::WiiU,
        }
    }
}

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Commands for working with filelists
    Filelist {
        #[command(subcommand)]
        subcommand: FilelistCommand,
    },
    Edb {
        #[command(subcommand)]
        subcommand: EdbCommand,
    },
}

#[derive(Subcommand, Debug, Clone)]
enum EdbCommand {
    /// Extract entities
    Entities {
        /// .edb file to read
        filename: String,

        /// Output folder for textures (default: "./entities/{filename}/")
        output_folder: Option<String>,

        /// Override for platform detection
        #[arg(value_enum, short, long, ignore_case = true)]
        platform: Option<PlatformArg>,

        /// Don't embed textures into the output file
        #[arg(short = 'e', long)]
        no_embed: bool,

        /// Remove transparent surfaces
        #[arg(short = 't', long)]
        no_transparent: bool,
    },
    /// Extract spreadsheets
    Spreadsheets {
        /// .edb file to read
        filename: String,

        /// Output folder for spreadsheet (default: "./spreadsheets/{filename}/")
        output_folder: Option<String>,
    },
    /// Extract maps
    Maps {
        /// .edb file to read
        filename: String,

        /// Output folder for maps (default: "./maps/{filename}/")
        output_folder: Option<String>,

        /// Override for platform detection
        #[arg(value_enum, short, long, ignore_case = true)]
        platform: Option<PlatformArg>,

        /// File with trigger definitions (assets/triggers_*.yml)
        #[arg(short, long)]
        trigger_defs: Option<String>,
    },
    /// Extract textures
    Textures {
        /// .edb file to read
        filename: String,

        /// Output folder for textures (default: "./textures/{filename}/")
        output_folder: Option<String>,

        /// Override for platform detection
        #[arg(value_enum, short, long, ignore_case = true)]
        platform: Option<PlatformArg>,

        /// Output file format to use (supported: tga, png, qoi)
        /// Selecting PNG will export animated textures as APNGs (unless disabled)
        #[arg(short, long, default_value("tga"))]
        format: String,

        /// Don't export APNGs when using PNG as output format
        #[arg(long)]
        no_apngs: bool,
    },
    /// Extract animations (!!MAJOR WIP!!)
    Animations {
        /// .edb file to read
        filename: String,

        /// Output folder for textures (default: "./entities/{filename}/")
        output_folder: Option<String>,

        // TODO(cohae): can we move this up to the edb command?
        /// Override for platform detection
        #[arg(value_enum, short, long, ignore_case = true)]
        platform: Option<PlatformArg>,
    },
    /// Patch text in an .edb file
    PatchText {
        /// .edb file to patch
        filename: String,

        /// CSV file with new text
        csv_file: String,

        /// Output .edb file (optional, will overwrite if not specified)
        output_filename: Option<String>,

        /// Target Set ID to patch (e.g. 08010000). All other sets will be wiped.
        #[arg(long, short, value_parser = maybe_hex::<u32>)]
        set: Option<u32>,
    },
    /// Find font addresses and info
    FindFonts {
        /// .edb file to read
        filename: String,
    },
    /// Dump texture offsets and properties
    DumpTextures {
        /// .edb file to read
        filename: String,
    },
    /// Inject textures from a directory back into the EDB
    InjectTextures {
        /// .edb file to patch
        filename: String,
        /// Directory containing png files
        textures_dir: String,
    },
    /// Dump font glyph pointers for debugging
    DumpFontGlyphs {
        /// .edb file to read
        filename: String,
        /// Font index to dump (default: 0)
        #[arg(short, long, default_value_t = 0)]
        font: usize,
    },
}

#[derive(Subcommand, Debug, Clone)]
enum FilelistCommand {
    /// Extract a filelist
    Extract {
        /// .bin file to use (don't use a .000 file)
        filename: String,

        /// The folder to extract to (will be created if it doesnt exist)
        #[arg(default_value = "./")]
        output_folder: String,

        /// Create a .scr file in the output folder listing the contents in the right order, for future repacking
        #[arg(short = 's', long)]
        create_scr: bool,
    },
    /// Create a new filelist from a folder
    Create {
        /// Folder to read files from
        input_folder: String,

        /// Destination for the generated filelist (without filename extension)
        #[arg(default_value = "./Filelist")]
        output_file: String,

        #[arg(long, short = 'l', default_value_t = 'x')]
        drive_letter: char,

        /// Supported versions: 5, 6, 7
        #[arg(long, short, default_value_t = 7)]
        version: u32,

        #[arg(value_enum, short, long, ignore_case = true)]
        platform: PlatformArg,

        /// Maximum size per data file, might be overridden by a .scr file
        #[arg(long, short = 'z', default_value_t = 0x80000000, value_parser = maybe_hex::<u32>)]
        split_size: u32,

        /// .scr file to read options from (currently doesnt support wildcards)
        #[arg(long, short)]
        scr_file: Option<String>,
    },
}

pub fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().without_time())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .with_env_var("EUROCHEF_LOG")
                .from_env_lossy(),
        )
        .init();

    let args = Args::parse();

    match &args.cmd {
        Command::Filelist { subcommand } => handle_filelist(subcommand.clone()),
        Command::Edb { subcommand } => handle_edb(subcommand.clone()),
    }
}

fn handle_edb(cmd: EdbCommand) -> anyhow::Result<()> {
    match cmd {
        EdbCommand::Entities {
            filename,
            output_folder,
            platform,
            no_embed,
            no_transparent,
        } => edb::entities::execute_command(
            filename,
            platform,
            output_folder,
            no_embed,
            no_transparent,
        ),
        EdbCommand::Maps {
            filename,
            platform,
            output_folder,
            trigger_defs,
        } => edb::maps::execute_command(filename, platform, output_folder, trigger_defs),
        EdbCommand::Spreadsheets {
            filename,
            output_folder,
        } => edb::spreadsheets::execute_command(filename, output_folder),
        EdbCommand::Textures {
            filename,
            platform,
            output_folder,
            format,
            no_apngs,
        } => edb::textures::execute_command(filename, platform, output_folder, format, no_apngs),
        EdbCommand::Animations {
            filename,
            platform,
            output_folder,
        } => edb::animations::execute_command(filename, platform, output_folder),
        EdbCommand::PatchText {
            filename,
            csv_file,
            output_filename,
            set,
        } => edb::patch::execute_patch_text(filename, csv_file, output_filename, set),
        EdbCommand::FindFonts { filename } => find_fonts(filename),
        EdbCommand::DumpTextures { filename } => dump_textures(filename),
        EdbCommand::InjectTextures { filename, textures_dir } => inject_textures(filename, textures_dir),
        EdbCommand::DumpFontGlyphs { filename, font } => dump_font_glyphs(filename, font),
    }
}

fn find_fonts(filename: String) -> anyhow::Result<()> {
    let f = File::open(&filename)?;
    let platform = Platform::from_path(&filename).unwrap();
    let mut edb = EdbFile::new(Box::new(BufReader::new(f)), platform)?;
    
    let font_list = edb.header.font_list.clone();
    for (i, font) in font_list.iter().enumerate() {
        edb.seek(SeekFrom::Start(font.address as u64))?;
        let texture_index = edb.read_type::<u32>(edb.endian)?;
        let glyph_count = edb.read_type::<u32>(edb.endian)?;
        println!("Font {}: hash={:08x}, addr={:08x}, texture_index={}, glyph_count={}", i, font.hashcode, font.address, texture_index, glyph_count);
    }
    
    Ok(())
}

fn dump_textures(filename: String) -> anyhow::Result<()> {
    use eurochef_edb::texture::EXGeoTexture;

    let f = File::open(&filename)?;
    let platform = Platform::from_path(&filename).unwrap();
    let mut edb = EdbFile::new(Box::new(BufReader::new(f)), platform)?;
    
    for tex_ptr in edb.header.texture_list.clone().iter() {
        let hashcode = tex_ptr.common.hashcode;
        edb.seek(SeekFrom::Start(tex_ptr.common.address as u64))?;
        let tex = edb.read_type_args::<EXGeoTexture>(edb.endian, (edb.header.version, edb.platform))?;
        
        let mut mips = Vec::new();
        for offset in tex.frame_offsets.iter() {
            mips.push(offset.offset_absolute());
        }
        
        println!("{:08x},{:?},{},{},{}", hashcode, mips, tex.format, tex.width, tex.height);
    }
    
    Ok(())
}

fn inject_textures(filename: String, textures_dir: String) -> anyhow::Result<()> {
    use eurochef_edb::texture::EXGeoTexture;
    use std::fs::OpenOptions;
    use std::io::Write;
    use image::GenericImageView;
    use image::imageops::FilterType;

    let mut file = OpenOptions::new().read(true).write(true).open(&filename)?;
    let platform = Platform::from_path(&filename).unwrap();
    let mut edb = EdbFile::new(Box::new(BufReader::new(file.try_clone()?)), platform)?;
    
    let textures_dir = std::path::Path::new(&textures_dir);

    for tex_ptr in edb.header.texture_list.clone().iter() {
        let hashcode = tex_ptr.common.hashcode;
        let mut img_path = textures_dir.join(format!("{:08x}.png", hashcode));
        
        if !img_path.exists() {
            img_path = textures_dir.join(format!("{:08x}_frame0.png", hashcode));
        }

        if !img_path.exists() {
            img_path = textures_dir.join(format!("{:08x}.tga", hashcode));
        }

        if !img_path.exists() {
            img_path = textures_dir.join(format!("{:08x}_frame0.tga", hashcode));
        }

        if !img_path.exists() {
            continue;
        }

        println!("Injecting {:?}...", img_path.file_name().unwrap());

        edb.seek(SeekFrom::Start(tex_ptr.common.address as u64))?;
        let tex = edb.read_type_args::<EXGeoTexture>(edb.endian, (edb.header.version, edb.platform))?;
        
        let base_img = image::open(&img_path)?;
        let mips = tex.frame_offsets.len();

        for i in 0..mips {
            let offset = tex.frame_offsets[i].offset_absolute();
            let mut w = std::cmp::max(1, tex.width >> i) as u32;
            let mut h = std::cmp::max(1, tex.height >> i) as u32;

            let mip_img = if i == 0 {
                base_img.clone()
            } else {
                base_img.resize_exact(w, h, FilterType::Lanczos3)
            };

            let mut out_data = Vec::new();

            match tex.format {
                0 => { // RGB565
                    let mip_img = mip_img.to_rgb8();
                    for y in 0..h {
                        for x in 0..w {
                            let p = mip_img.get_pixel(x, y);
                            let r = p[0] as u16;
                            let g = p[1] as u16;
                            let b = p[2] as u16;
                            let val = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
                            out_data.extend_from_slice(&val.to_le_bytes());
                        }
                    }
                },
                6 => { // ARGB8 (BGRA in memory for PC)
                    let mip_img = mip_img.to_rgba8();
                    for y in 0..h {
                        for x in 0..w {
                            let p = mip_img.get_pixel(x, y);
                            out_data.push(p[2]); // B
                            out_data.push(p[1]); // G
                            out_data.push(p[0]); // R
                            out_data.push(p[3]); // A
                        }
                    }
                },
                2 | 3 | 4 | 7 | 8 | 9 => { // DXT1, DXT1Alpha, DXT2, DXT3, DXT4, DXT5
                    let mip_img = mip_img.to_rgba8();
                    let bcn = match tex.format {
                        2 | 3 => squish::Format::Bc1,
                        4 | 7 => squish::Format::Bc2,
                        8 | 9 => squish::Format::Bc3,
                        _ => unreachable!()
                    };
                    // DXT expects dimensions to be multiple of 4, or it handles it? squish handles any size but padding is usually done
                    let mut squish_out = vec![0u8; bcn.compressed_size(w as usize, h as usize)];
                    let params = squish::Params {
                        algorithm: squish::Algorithm::IterativeClusterFit,
                        weights: [1.0, 1.0, 1.0],
                        weigh_colour_by_alpha: false,
                    };
                    bcn.compress(mip_img.as_raw(), w as usize, h as usize, params, &mut squish_out);
                    out_data = squish_out;
                },
                _ => {
                    println!("Skipping unsupported format {} for {:08x}", tex.format, hashcode);
                    continue;
                }
            }

            file.seek(SeekFrom::Start(offset as u64))?;
            file.write_all(&out_data)?;
        }
    }
    
    Ok(())
}


fn dump_font_glyphs(filename: String, font_index: usize) -> anyhow::Result<()> {
    use std::fs::File;
    use std::io::{BufReader, Read, Seek, SeekFrom};

    let f = File::open(&filename)?;
    let platform = Platform::from_path(&filename).unwrap();
    let mut edb = EdbFile::new(Box::new(BufReader::new(f)), platform)?;
    
    let font = edb.header.font_list.iter().nth(font_index).context("Font index out of bounds")?;
    let font_base = font.address as u64;
    
    let mut file = File::open(&filename)?;
    file.seek(SeekFrom::Start(font_base))?;
    let mut header_buf = [0u8; 32];
    file.read_exact(&mut header_buf)?;
    
    let ptr_array_rel_offset = if edb.endian == eurochef_edb::binrw::Endian::Little {
        u32::from_le_bytes(header_buf[8..12].try_into().unwrap())
    } else {
        u32::from_be_bytes(header_buf[8..12].try_into().unwrap())
    };

    let ptr_array_abs = font_base + (ptr_array_rel_offset as u64) + 8;
    
    println!("Dumping pointers for Font {} at 0x{:x} (array at 0x{:x}):", font_index, font_base, ptr_array_abs);
    
    // Dump indices around Cyrillic range
    for i in 250..400 {
        file.seek(SeekFrom::Start(ptr_array_abs + (i as u64) * 4))?;
        let mut ptr_buf = [0u8; 4];
        file.read_exact(&mut ptr_buf)?;
        let ptr = if edb.endian == eurochef_edb::binrw::Endian::Little {
            u32::from_le_bytes(ptr_buf)
        } else {
            u32::from_be_bytes(ptr_buf)
        };
        
        // Print index, pointer, and if it matches a known "victim" pointer we might recognize
        print!("{:3}: 0x{:08x}  ", i, ptr);
        if (i + 1) % 4 == 0 { println!(); }
    }
    println!();
    
    Ok(())
}

fn handle_filelist(cmd: FilelistCommand) -> anyhow::Result<()> {
    match cmd {
        FilelistCommand::Extract {
            filename,
            output_folder,
            create_scr,
        } => filelist::extract::execute_command(filename, output_folder, create_scr)
            .context("Failed to extract filelist"),
        FilelistCommand::Create {
            input_folder,
            output_file,
            drive_letter,
            version,
            platform,
            split_size,
            scr_file,
        } => filelist::create::execute_command(
            input_folder,
            output_file,
            drive_letter,
            version,
            platform,
            split_size,
            scr_file,
        )
        .context("Failed to create filelist"),
    }
}
