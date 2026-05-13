# OSS-Fuzz Integration Guide

This document provides step-by-step instructions for integrating inverter-desktop with [OSS-Fuzz](https://google.github.io/oss-fuzz/).

## Prerequisites

- GitHub account with admin access to victron-venus/inverter-desktop
- Google account for OSS-Fuzz
- Basic understanding of Docker and fuzzing concepts

## Step 1: Prepare Project Files

The following files are already prepared for OSS-Fuzz submission:

- `oss-fuzz.json` - Project configuration
- `Dockerfile` - Build environment
- `build.sh` - Build script
- `src-tauri/fuzz/` - Fuzz targets and corpus

## Step 2: Submit to OSS-Fuzz

### 2.1 Fork OSS-Fuzz Repository

```bash
# Fork https://github.com/google/oss-fuzz
git clone https://github.com/YOUR_USERNAME/oss-fuzz.git
cd oss-fuzz
```

### 2.2 Create Project Directory

```bash
mkdir projects/inverter-desktop
cd projects/inverter-desktop
```

### 2.3 Copy Project Files

```bash
# Copy from your inverter-desktop repository
cp /path/to/inverter-desktop/Dockerfile .
cp /path/to/inverter-desktop/build.sh .
cp /path/to/inverter-desktop/oss-fuzz.json .

# Create project structure
mkdir -p src
cd src
git clone https://github.com/victron-venus/inverter-desktop.git
```

### 2.4 Update Dockerfile

Edit `Dockerfile` to use the correct source path:

```dockerfile
# Copy project files
COPY . /src
WORKDIR /src

# Copy inverter-desktop source
COPY src/inverter-desktop /src/inverter-desktop
WORKDIR /src/inverter-desktop
```

### 2.5 Test Locally

```bash
# From oss-fuzz root directory
python3 infra/helper.py build_image inverter-desktop
python3 infra/helper.py build_fuzzers inverter-desktop
python3 infra/helper.py run_fuzzer inverter-desktop fuzz_json_parsing
```

### 2.6 Commit and Push

```bash
git add projects/inverter-desktop
git commit -m "Add inverter-desktop project"
git push origin main
```

## Step 3: Submit Pull Request

1. Go to https://github.com/google/oss-fuzz
2. Create Pull Request from your fork
3. Use title: "Add inverter-desktop project"
4. Include description:

```
## Project Description

inverter-desktop is a Tauri-based desktop application for monitoring Victron inverter systems via MQTT.

## Fuzzing Details

- **Language**: Rust
- **Fuzz Targets**: 3 (JSON parsing, MQTT handling, command parsing)
- **Sanitizers**: address, undefined
- **Engines**: libfuzzer, AFL

## Testing

Tested locally with:
```bash
python3 infra/helper.py build_image inverter-desktop
python3 infra/helper.py build_fuzzers inverter-desktop
python3 infra/helper.py run_fuzzer inverter-desktop fuzz_json_parsing
```

## Additional Information

- Homepage: https://github.com/victron-venus/inverter-desktop
- Maintainer: @4alvit
- License: MIT
```

## Step 4: Monitor and Respond

### 4.1 Review Process

- OSS-Fuzz maintainers will review your PR
- They may request changes or improvements
- Typical review time: 1-2 weeks

### 4.2 Common Issues

**Build Failures:**
- Check Dockerfile syntax
- Verify build.sh permissions
- Ensure all dependencies are available

**Runtime Issues:**
- Test fuzz targets locally first
- Check corpus files are valid
- Verify dictionary format

**Configuration Issues:**
- Validate oss-fuzz.json format
- Check project name uniqueness
- Verify maintainer email

### 4.3 Approval and Integration

Once approved:
- Project will be added to OSS-Fuzz
- Continuous fuzzing will begin
- Crash reports will be sent to maintainers

## Step 5: Post-Integration Setup

### 5.1 Configure Notifications

Add yourself to project maintainers in oss-fuzz.json:

```json
{
  "auto_ccs": ["4alvit", "your-email@example.com"]
}
```

### 5.2 Monitor Results

- Check OSS-Fuzz dashboard for findings
- Review crash reports
- Fix vulnerabilities promptly

### 5.3 Update Documentation

- Update README.md with OSS-Fuzz badge
- Document fuzzing findings in SECURITY_STATUS.md
- Share results with community

## OSS-Fuzz Dashboard

Once integrated, monitor:
- https://oss-fuzz.com/testcases?project=inverter-desktop
- https://introspector.oss-fuzz.com/?project=inverter-desktop

## Troubleshooting

### Build Issues

```bash
# Check build logs
python3 infra/helper.py build_image inverter-desktop --verbose

# Test build locally
docker build -t inverter-desktop-fuzz .
```

### Runtime Issues

```bash
# Run with debug output
python3 infra/helper.py run_fuzzer inverter-desktop fuzz_json_parsing -- -verbosity=2

# Check corpus
ls -la $OUT/corpus/
```

### Integration Issues

- Check OSS-Fuzz documentation: https://google.github.io/oss-fuzz/
- Review existing Rust projects for examples
- Contact OSS-Fuzz maintainers via GitHub issues

## Best Practices

### Continuous Improvement

1. **Add New Fuzz Targets**: As features are added
2. **Update Corpus**: With interesting test cases
3. **Monitor Coverage**: Using OSS-Fuzz introspection
4. **Fix Findings**: Promptly address vulnerabilities

### Community Engagement

- Share fuzzing results
- Contribute improvements to OSS-Fuzz
- Help other projects with fuzzing

## Resources

- [OSS-Fuzz Documentation](https://google.github.io/oss-fuzz/)
- [OSS-Fuzz GitHub](https://github.com/google/oss-fuzz)
- [Fuzzing Best Practices](https://github.com/google/oss-fuzz/blob/master/docs/ideal_integration.md)
- [ClusterFuzz Documentation](https://google.github.io/clusterfuzzlite/)

## Support

For issues specific to:
- **OSS-Fuzz integration**: Open issue in google/oss-fuzz
- **Fuzzing questions**: Contact OSS-Fuzz maintainers
- **Project issues**: Open issue in victron-venus/inverter-desktop

## License

OSS-Fuzz integration follows the same MIT license as the main project.
