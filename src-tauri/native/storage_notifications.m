// Fixed local notifications only; no URLs, attachments, sounds or file actions.
#import <Foundation/Foundation.h>
#import <UserNotifications/UserNotifications.h>
#include <stdlib.h>
#include <string.h>

static NSDictionary *failure(NSString *code) { return @{ @"error": code }; }
static NSDictionary *settings(UNNotificationSettings *s) {
    NSString *authorization = @"unknown";
    switch (s.authorizationStatus) {
        case UNAuthorizationStatusNotDetermined: authorization = @"notDetermined"; break;
        case UNAuthorizationStatusDenied: authorization = @"denied"; break;
        case UNAuthorizationStatusAuthorized: authorization = @"authorized"; break;
        case UNAuthorizationStatusProvisional: authorization = @"provisional"; break;
        default: break;
    }
    return @{ @"authorization": authorization,
              @"alerts": (s.alertSetting == UNNotificationSettingEnabled ? @YES : @NO),
              @"center": (s.notificationCenterSetting == UNNotificationSettingEnabled ? @YES : @NO) };
}
static NSDictionary *readSettings(UNUserNotificationCenter *center) {
    dispatch_semaphore_t signal = dispatch_semaphore_create(0);
    // Callback-owned data survives timeouts; never capture stack pointers.
    __block NSDictionary *result = nil;
    [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *s) {
        result = settings(s);
        dispatch_semaphore_signal(signal);
    }];
    if (dispatch_semaphore_wait(signal, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC)) != 0)
        return failure(@"settingsTimeout");
    return result ?: failure(@"settingsUnknown");
}
char *moomole_storage_notification(const char *operation, const char *bundleId, const char *identifier, const char *title, const char *body) {
    @autoreleasepool {
        NSDictionary *result;
        @try {
            NSBundle *bundle = NSBundle.mainBundle;
            NSString *expected = bundleId ? [NSString stringWithUTF8String:bundleId] : nil;
            if (!expected.length || ![bundle.bundleIdentifier isEqualToString:expected] ||
                ![bundle.bundlePath.pathExtension isEqualToString:@"app"] || NSThread.isMainThread) {
                result = failure(@"requiresPackagedApp");
            } else {
                UNUserNotificationCenter *center = UNUserNotificationCenter.currentNotificationCenter;
                NSString *action = operation ? [NSString stringWithUTF8String:operation] : @"";
                if ([action isEqualToString:@"read"]) result = readSettings(center);
                else if ([action isEqualToString:@"request"]) {
                    dispatch_semaphore_t signal = dispatch_semaphore_create(0);
                    __block BOOL succeeded = NO;
                    [center requestAuthorizationWithOptions:UNAuthorizationOptionAlert completionHandler:^(BOOL granted, NSError *error) {
                        (void)granted;
                        succeeded = error == nil;
                        dispatch_semaphore_signal(signal);
                    }];
                    if (dispatch_semaphore_wait(signal, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC)) != 0)
                        result = failure(@"authorizationTimeout");
                    else result = succeeded ? readSettings(center) : failure(@"authorizationFailed");
                } else if ([action isEqualToString:@"send"]) {
                    NSDictionary *current = readSettings(center);
                    BOOL allowed = [current[@"authorization"] isEqual:@"authorized"] || [current[@"authorization"] isEqual:@"provisional"];
                    if (current[@"error"]) result = current;
                    else if (!allowed || (![current[@"alerts"] boolValue] && ![current[@"center"] boolValue])) result = failure(@"permissionDenied");
                    else if (!identifier || !title || !body) result = failure(@"invalidRequest");
                    else {
                        UNMutableNotificationContent *content = [UNMutableNotificationContent new];
                        content.title = [NSString stringWithUTF8String:title];
                        content.body = [NSString stringWithUTF8String:body];
                        UNNotificationRequest *request = [UNNotificationRequest requestWithIdentifier:[NSString stringWithUTF8String:identifier] content:content trigger:nil];
                        dispatch_semaphore_t signal = dispatch_semaphore_create(0);
                        __block BOOL accepted = NO;
                        [center addNotificationRequest:request withCompletionHandler:^(NSError *error) {
                            accepted = error == nil;
                            dispatch_semaphore_signal(signal);
                        }];
                        if (dispatch_semaphore_wait(signal, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC)) != 0) result = failure(@"submissionTimeout");
                        else result = accepted ? @{ @"submitted": @YES } : failure(@"submissionFailed");
                    }
                } else result = failure(@"invalidRequest");
            }
        } @catch (NSException *exception) { result = failure(@"nativeUnavailable"); }
        NSData *json = [NSJSONSerialization dataWithJSONObject:result ?: failure(@"nativeUnavailable") options:0 error:nil];
        if (!json || json.length > 1024) return NULL;
        char *output = malloc(json.length + 1);
        if (output) { memcpy(output, json.bytes, json.length); output[json.length] = 0; }
        return output;
    }
}
