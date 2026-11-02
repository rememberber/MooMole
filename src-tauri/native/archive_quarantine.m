#import <Foundation/Foundation.h>
#import <CoreServices/CoreServices.h>

// Only called for a Rust-owned file in a private, automatically removed tempdir.
// Let Launch Services create a new download marker; never replay archived approval.
int moomole_archive_quarantine(const char *path) {
    @autoreleasepool {
        NSString *name = [[NSString alloc] initWithUTF8String:path];
        if (name == nil) return 0;
        NSURL *url = [NSURL fileURLWithPath:name];
        NSDictionary *properties = @{
            (__bridge NSString *)kLSQuarantineAgentNameKey: @"MooMole",
            (__bridge NSString *)kLSQuarantineAgentBundleIdentifierKey: @"io.github.rememberber.moomole",
            (__bridge NSString *)kLSQuarantineTypeKey: (__bridge NSString *)kLSQuarantineTypeOtherDownload
        };
        NSError *error = nil;
        return [url setResourceValue:properties forKey:NSURLQuarantinePropertiesKey error:&error] ? 1 : 0;
    }
}
