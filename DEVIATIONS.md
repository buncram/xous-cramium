# Deviations from main-line

- There is a `cramium` target added in the kernel and loader
  - As of July 2023 it only targets the DUART for console output, and the kernel has no UART interface.
- The image creation tool has significant changes.
  - the loader needs to know what memory regions are valid, so it can white-list them to the kernel
  - these memory regions are taken as a form of arguments to the loader, generated by the image creation tool
  - create-image.rs can now merge multiple SVD files together (this change is not in the mainline!)
  - it will take multiple small memory regions and merge them before handing them off to the loader
  - this is necessary because the SoC is generated using disjoint design methodologies, each creating a different SVD with sometimes overlapping regions